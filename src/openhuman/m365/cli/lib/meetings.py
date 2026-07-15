import json
import base64
import time
import urllib.request
import urllib.parse

from .api import graph_request
from .tokens import ensure_token, ensure_spo_token, ensure_substrate_token, load_tokens


def _graph_chat_request(endpoint):
    """Make a Graph API request using the graph_chat token (includes Chat.Read scope)."""
    tokens = load_tokens()
    graph_chat_token = (tokens.get('graph_chat') or {}).get('token', '')
    if not graph_chat_token:
        # fallback to graph token
        return graph_request(endpoint, raw=True)
    url = f'https://graph.microsoft.com/v1.0/{endpoint}'
    req = urllib.request.Request(url, headers={'Authorization': f'Bearer {graph_chat_token}'})
    try:
        with urllib.request.urlopen(req, timeout=30) as r:
            return {'status_code': r.status, 'body': json.load(r)}
    except urllib.error.HTTPError as e:
        import json as _json
        raw = e.read().decode('utf-8')
        try:
            body = _json.loads(raw)
        except Exception:
            body = raw
        return {'status_code': e.code, 'body': body}


def ticks_to_datetime(ticks):
    """Convert .NET ticks to Python datetime (UTC)."""
    if not ticks:
        return None
    from datetime import datetime, timezone
    t = int(ticks)
    # .NET epoch diff: ticks from 0001-01-01 to 1970-01-01
    epoch_diff = 621355968000000000
    ms = (t - epoch_diff) // 10000
    return datetime.fromtimestamp(ms / 1000, tz=timezone.utc)


def json_fetch(url, method='GET', headers=None, body=None, timeout=30):
    data = None
    if body:
        data = (json.dumps(body) if not isinstance(body, str) else body).encode('utf-8')
    req = urllib.request.Request(url, data=data, headers=headers or {}, method=method)
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            content_type = resp.headers.get('Content-Type', '')
            raw = resp.read().decode('utf-8')
            body_parsed = json.loads(raw) if 'json' in content_type else raw
            return {'status_code': resp.status, 'body': body_parsed}
    except urllib.error.HTTPError as e:
        content_type = e.headers.get('Content-Type', '')
        raw = e.read().decode('utf-8')
        body_parsed = json.loads(raw) if 'json' in content_type else raw
        return {'status_code': e.code, 'body': body_parsed}


def get_meeting_artifacts(chat_id):
    chat_result = _graph_chat_request(f'chats/{urllib.parse.quote(chat_id, safe="")}?$select=id,chatType,tenantId,onlineMeetingInfo')
    if chat_result['status_code'] != 200:
        raise RuntimeError(f"Failed to get chat: {chat_result['status_code']} {json.dumps(chat_result['body'])}")

    chat = chat_result['body']
    if not chat.get('onlineMeetingInfo'):
        raise RuntimeError('Not a meeting chat (no onlineMeetingInfo)')

    tenant_id = chat.get('tenantId')
    organizer_id = (chat.get('onlineMeetingInfo') or {}).get('organizer', {}).get('id')
    if not tenant_id or not organizer_id:
        raise RuntimeError('Missing tenantId or organizerId in chat metadata')

    thread_id = chat_id
    org_id_full = f'{organizer_id}@{tenant_id}'

    teams_token = ensure_token('teams')
    mcps_url = f'https://teams.microsoft.com/api/mcps/eu/contents/?threadId={urllib.parse.quote(thread_id, safe="")}&organizerId={urllib.parse.quote(org_id_full, safe="")}'

    mcps_resp = json_fetch(mcps_url, headers={
        'authorization': f'Bearer {teams_token}',
        'x-ms-caller-name': 'RecapNative',
        'content-type': 'application/json;charset=UTF-8',
    })

    if mcps_resp['status_code'] != 200:
        raise RuntimeError(f"MCPS API error: {mcps_resp['status_code']}")

    body = mcps_resp['body']
    resources = body.get('resources') or body.get('value') or []
    if not isinstance(resources, list):
        raise RuntimeError('Unexpected MCPS response format')

    # Group by callId
    instances = {}
    for r in resources:
        meta = r.get('metadata') or {}
        call_id = meta.get('callId')
        if not call_id:
            continue
        if call_id not in instances:
            instances[call_id] = {'callId': call_id, 'startTime': None, 'endTime': None, 'artifacts': {}}
        inst = instances[call_id]

        start = ticks_to_datetime(meta.get('startTime'))
        end = ticks_to_datetime(meta.get('endTime'))
        if start and (not inst['startTime'] or start < inst['startTime']):
            inst['startTime'] = start
        if end and (not inst['endTime'] or end > inst['endTime']):
            inst['endTime'] = end

        art_type = r.get('type') or 'unknown'
        inst['artifacts'][art_type] = r

    sorted_instances = sorted(
        instances.values(),
        key=lambda x: x['startTime'] or __import__('datetime').datetime.min.replace(tzinfo=__import__('datetime').timezone.utc),
        reverse=True,
    )

    return {'threadId': thread_id, 'organizerId': org_id_full, 'tenantId': tenant_id, 'instances': sorted_instances}


def get_transcript(chat_id, call_id=None):
    result = get_meeting_artifacts(chat_id)
    instances = result['instances']
    if not instances:
        raise RuntimeError('No meeting instances found')

    if call_id:
        instance = next((i for i in instances if i['callId'] == call_id), None)
        if not instance:
            raise RuntimeError(f'Meeting instance with callId {call_id} not found')
    else:
        instance = instances[0]

    transcript = instance['artifacts'].get('TranscriptV2') or instance['artifacts'].get('Transcript')
    if not transcript:
        raise RuntimeError(f"No transcript found for meeting instance {instance['callId']}")

    location = transcript.get('location')
    if not location:
        raise RuntimeError('Transcript resource has no location URL')

    stream_url = location.rstrip('/content') + '/streamContent' if location.endswith('/content') else location + '/streamContent'
    stream_url = location.replace('/content', '/streamContent') + '?format=json&applyhighlights=false&applymediaedits=false'

    from urllib.parse import urlparse
    spo_host = urlparse(location).hostname
    spo_token = ensure_spo_token(spo_host)

    spo_resp = json_fetch(stream_url, headers={
        'authorization': f'Bearer {spo_token}',
        'accept': '*/*',
        'content-type': 'application/json',
    })

    if spo_resp['status_code'] != 200:
        raise RuntimeError(f"SPO API error: {spo_resp['status_code']}")

    transcript_body = spo_resp['body']
    entries = transcript_body.get('entries') or transcript_body.get('value') or []

    start_iso = instance['startTime'].isoformat() if instance['startTime'] else None
    end_iso = instance['endTime'].isoformat() if instance['endTime'] else None

    return {
        'callId': instance['callId'],
        'startTime': start_iso,
        'endTime': end_iso,
        'entries': [
            {
                'speaker': e.get('speakerDisplayName') or e.get('speaker') or 'Unknown',
                'text': e.get('text') or '',
                'startOffset': e.get('startOffset') or '',
                'endOffset': e.get('endOffset') or '',
            }
            for e in entries
        ],
    }


def get_summary(chat_id, call_id=None):
    result = get_meeting_artifacts(chat_id)
    instances = result['instances']
    tenant_id = result['tenantId']

    if not instances:
        raise RuntimeError('No meeting instances found')

    if call_id:
        instance = next((i for i in instances if i['callId'] == call_id), None)
        if not instance:
            raise RuntimeError(f'Meeting instance with callId {call_id} not found')
    else:
        instance = instances[0]

    ai_artifact = instance['artifacts'].get('AISummary')
    if not ai_artifact:
        raise RuntimeError(f"No AI summary available for meeting instance {instance['callId']}")

    spo_id = (ai_artifact.get('metadata') or {}).get('sharePointOnlineId')
    if not spo_id:
        raise RuntimeError('AISummary artifact has no sharePointOnlineId')

    graph_token = ensure_token('graph')
    oid = None
    try:
        payload_b64 = graph_token.split('.')[1]
        payload_b64 += '=' * (-len(payload_b64) % 4)
        payload = json.loads(base64.b64decode(payload_b64).decode('utf-8'))
        oid = payload.get('oid')
    except Exception:
        pass
    if not oid:
        raise RuntimeError('Cannot extract user OID from graph token')

    substrate_token = ensure_substrate_token()

    resp = json_fetch(
        'https://substrate.office.com/search/api/v1/recommendations/?&setflight=AiTasksV3,AiNotesV3,PeopleMentionsV2',
        method='POST',
        headers={
            'authorization': f'Bearer {substrate_token}',
            'x-anchormailbox': f'Oid:{oid}@{tenant_id}',
            'accept': 'application/json',
            'content-type': 'application/json',
            'Referer': 'https://teams.microsoft.com/',
        },
        body={
            'EntityRequests': [{'Context': {'EntityId': spo_id}, 'QueryParameters': [{'EntityType': 'MeetingCatchUp'}]}],
            'Scenario': {'Name': 'MeetingCatchUp.MeetingRecap'},
        },
    )

    if resp['status_code'] != 200:
        raise RuntimeError(f"Substrate API error: {resp['status_code']}")

    result_item = ((resp['body'].get('EntitySets') or [{}])[0].get('ResultSets') or [{}])[0].get('Results') or [None]
    result_item = result_item[0] if result_item else None
    if not result_item:
        raise RuntimeError('No AI summary data returned')

    src = result_item.get('Source') or {}
    meeting_summary = src.get('MeetingSummary')
    if isinstance(meeting_summary, str):
        meeting_summary = json.loads(meeting_summary)
    points_of_interest = src.get('PointsOfInterest')
    if isinstance(points_of_interest, str):
        points_of_interest = json.loads(points_of_interest)

    start_iso = instance['startTime'].isoformat() if instance['startTime'] else None
    end_iso = instance['endTime'].isoformat() if instance['endTime'] else None

    return {
        'callId': instance['callId'],
        'startTime': start_iso,
        'endTime': end_iso,
        'title': src.get('Title') or src.get('MeetingSubject'),
        'topics': [
            {
                'headline': t.get('Headline'),
                'summary': t.get('Summary'),
                'details': [{'topic': d.get('topic'), 'text': d.get('text')} for d in (t.get('DetailedSummaries') or [])],
            }
            for t in ((meeting_summary or {}).get('KeyTopics') or [])
        ],
        'actionItems': [
            {'text': a.get('DisplayCleanContent') or a.get('Content') or ''}
            for a in ((points_of_interest or {}).get('ActionItems') or [])
        ],
    }
