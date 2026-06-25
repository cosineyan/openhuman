import re
import uuid
import base64
import json as _json
import urllib.parse
from datetime import datetime, timezone
import click
from ..tokens import ensure_token, ensure_substrate_token
from ..api import graph_request


def strip_html(html):
    if not html:
        return ''
    text = re.sub(r'<br\s*/?>', '\n', html, flags=re.IGNORECASE)
    text = re.sub(r'<[^>]+>', '', text)
    for ent, ch in [('&amp;', '&'), ('&lt;', '<'), ('&gt;', '>'), ('&quot;', '"'), ('&#39;', "'"), ('&nbsp;', ' ')]:
        text = text.replace(ent, ch)
    return text.strip()


def fmt_time(iso):
    if not iso:
        return ''
    try:
        d = datetime.fromisoformat(iso.replace('Z', '+00:00'))
        return d.strftime('%b %-d, %H:%M')
    except Exception:
        return iso[:16]


def get_user_info():
    graph_token = ensure_token('graph')
    try:
        padded = graph_token.split('.')[1]
        padded += '=' * (-len(padded) % 4)
        payload = _json.loads(base64.b64decode(padded).decode())
        return {'oid': payload.get('oid'), 'tenant_id': payload.get('tid'), 'puid': payload.get('puid'), 'upn': payload.get('upn')}
    except Exception:
        raise RuntimeError('Cannot extract user info from graph token')


def to_iso_date(s):
    if re.match(r'^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d+)?Z$', s):
        return s
    try:
        if re.match(r'^\d{4}-\d{2}-\d{2}$', s):
            return datetime.fromisoformat(s).replace(tzinfo=timezone.utc).isoformat().replace('+00:00', 'Z')
        d = datetime.fromisoformat(s.replace('Z', '+00:00'))
        return d.astimezone(timezone.utc).isoformat().replace('+00:00', 'Z')
    except Exception:
        raise ValueError(f'Invalid date: {s}')


def build_message_qs(query, chat=None, since=None, until=None, sender=None, mentions_me=False, oid=None):
    parts = []
    if since:
        parts.append(f'sent >= {since}')
    if until:
        parts.append(f'sent < {until}')
    if mentions_me:
        if not oid:
            raise RuntimeError('oid required for mentions_me filter')
        parts.append(f"mentions:{oid.replace('-', '')}")
    if chat:
        parts.append(f'clientthreadid:{chat}')
        parts.append('NOT (Extension_SkypeSpaces_ConversationPost_Extension_ThreadType_String:(topic OR space))')
    if sender:
        parts.append(sender['email'])
    if query:
        parts.append(query)
    return ' AND '.join(parts)


def build_message_request(query, size, from_, filters=None):
    filters = filters or {}
    qs = build_message_qs(query, **{k: filters.get(k) for k in ('chat', 'since', 'until', 'sender', 'mentions_me')}, oid=filters.get('oid'))
    qs_parts = ['NOT (isClientSoftDeleted:TRUE)']
    if qs:
        qs_parts.append(qs)
    query_obj = {'queryString': ' AND '.join(qs_parts), 'displayQueryString': query}
    if filters.get('sender'):
        s = filters['sender']
        query_obj['queryAnnotations'] = [{'type': 'People', 'text': s['email'], 'userId': s['userId'], 'tenantId': s['tenant_id'], 'subtype': ['Sharer']}]
    return {
        'entityType': 'Message',
        'contentSources': ['Teams'],
        'fields': [
            'Extension_SkypeSpaces_ConversationPost_Extension_FromSkypeInternalId_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_FileData_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_ThreadType_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_SkypeGroupId_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_SenderTenantId_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_ImageSrc_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_ParentMessageId_String',
            'Extension_SkypeSpaces_ConversationPost_Extension_AmsReferences_StringArray',
        ],
        'propertySet': 'Optimized',
        'query': query_obj,
        'from': from_,
        'size': size,
        'topResultsCount': 5,
    }


def build_people_request(query, size, from_, filters=None):
    return {
        'entityType': 'People',
        'contentSources': ['Exchange'],
        'Filter': {'And': [
            {'Or': [{'Term': {'Flags': 'NonHidden'}}]},
            {'Or': [{'Term': {'PeopleType': 'Person'}}, {'Term': {'PeopleType': 'Other'}}]},
            {'Or': [
                {'Term': {'PeopleSubtype': 'OrganizationUser'}},
                {'Term': {'PeopleSubtype': 'MTOUser'}},
                {'Term': {'PeopleSubtype': 'Guest'}},
                {'Term': {'PeopleSubtype': 'Room'}},
                {'Term': {'PeopleSubtype': 'PersonalContact'}},
                {'Term': {'PeopleSubtype': 'ImplicitContact'}},
            ]},
        ]},
        'query': {'queryString': query, 'displayQueryString': query},
        'from': from_,
        'size': size,
    }


def build_chat_request(query, size, from_, filters=None):
    return {
        'entityType': 'Chat',
        'contentSources': ['Teams'],
        'propertySet': 'Optimized',
        'fields': [],
        'query': {'queryString': query, 'displayQueryString': query},
        'extendedQueries': [{'query': {}}],
        'from': from_,
        'size': size,
    }


def build_channel_request(query, size, from_, filters=None):
    return {
        'entityType': 'TeamsChannel',
        'contentSources': ['Teams'],
        'HitHighlight': {'HitHighlightedProperties': ['HitHighlightedSummary'], 'SummaryLength': 200},
        'fields': [],
        'query': {'queryString': query, 'displayQueryString': query},
        'extendedQueries': [{'query': {}}],
        'from': from_,
        'size': size,
    }


def build_file_request(query, size, from_, filters=None):
    return {
        'contentSources': ['OneDriveBusiness', 'Exchange'],
        'EnableQueryUnderstanding': False,
        'EnableSpeller': False,
        'EntityType': 'File',
        'Fields': ['FileName', 'FileType', 'LastModifiedTime', 'ModifiedBy', 'Path', 'Title', 'SiteName', 'Size'],
        'From': from_,
        'PropertySet': 'Optimized',
        'Query': {'QueryString': query, 'DisplayQueryString': query},
        'size': size,
        'Sort': [{'Field': 'PersonalScore', 'SortDirection': 'Desc'}],
    }


def build_event_request(query, size, from_, filters=None):
    return {
        'Query': {'QueryString': query},
        'EntityTypes': ['Event'],
        'Size': size,
        'From': from_,
        'EnableAsyncResolution': True,
    }


ENTITY_BUILDERS = {
    'messages': build_message_request,
    'people': build_people_request,
    'chats': build_chat_request,
    'channels': build_channel_request,
    'files': build_file_request,
    'events': build_event_request,
}
VALID_TYPES = list(ENTITY_BUILDERS.keys())


def resolve_person(email):
    info = get_user_info()
    result = graph_request(f'users/{urllib.parse.quote(email)}?$select=id', raw=True)
    if result['status_code'] != 200:
        raise RuntimeError(f'Cannot resolve user "{email}": {result["status_code"]} {result["body"]}')
    user_id = (result['body'] or {}).get('id')
    if not user_id:
        raise RuntimeError(f'Cannot resolve userId for: {email}')
    return {'email': email, 'userId': user_id, 'tenant_id': info['tenant_id']}


class _SubstrateUnauthorized(Exception):
    pass


def substrate_search(query, entity_types, size, from_, filters=None):
    try:
        return _substrate_search_once(query, entity_types, size, from_, filters)
    except _SubstrateUnauthorized:
        from ..tokens import load_tokens, save_tokens
        t = load_tokens()
        t.pop('substrate', None)
        save_tokens(t)
        return _substrate_search_once(query, entity_types, size, from_, filters)


def _substrate_search_once(query, entity_types, size, from_, filters=None):
    import urllib.request as _req
    import urllib.error as _err
    filters = filters or {}
    substrate_token = ensure_substrate_token()
    info = get_user_info()
    oid = info['oid']
    tenant_id = info['tenant_id']
    puid = info['puid']

    entity_requests = [ENTITY_BUILDERS[t](query, size, from_, {**filters, 'oid': oid}) for t in entity_types]
    answer_reqs = [r for r in entity_requests if r.get('EntityTypes')]
    normal_reqs = [r for r in entity_requests if not r.get('EntityTypes')]

    body = {
        'EntityRequests': normal_reqs,
        'QueryAlterationOptions': {'EnableAlteration': True, 'EnableSuggestion': True},
        'cvid': str(uuid.uuid4()),
        'logicalId': str(uuid.uuid4()),
        'scenario': {
            'Dimensions': [
                {'DimensionName': 'QueryType', 'DimensionValue': 'All'},
                {'DimensionName': 'FormFactor', 'DimensionValue': 'general.web.reactSearch'},
            ],
            'Name': 'powerbar',
        },
    }
    if answer_reqs:
        body['AnswerEntityRequests'] = answer_reqs

    data = _json.dumps(body).encode()
    req = urllib.request.Request(
        'https://substrate.office.com/searchservice/api/v2/query',
        data=data,
        method='POST',
        headers={
            'Authorization': f'Bearer {substrate_token}',
            'Content-Type': 'application/json',
            'X-AnchorMailbox': f'PUID:{puid}@{tenant_id}',
            'Origin': 'https://teams.microsoft.com',
            'Referer': 'https://teams.microsoft.com/',
            'X-Client-Version': 'T2.1',
        },
    )
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return _json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        if e.code == 401:
            raise _SubstrateUnauthorized()
        body_text = e.read().decode()
        raise RuntimeError(f'Substrate search API error: {e.code} {body_text}')


# --- Formatters ---

def format_message(hit, text_fn):
    src = hit.get('Source') or {}
    fr = (src.get('From') or {}).get('EmailAddress') or (src.get('Sender') or {}).get('EmailAddress') or {}
    sender = fr.get('Name') or fr.get('Address') or 'unknown'
    time = fmt_time(src.get('DateTimeReceived') or src.get('DateTimeSent'))
    preview = strip_html(src.get('Preview') or hit.get('HitHighlightedSummary') or '')
    thread_id = src.get('ClientThreadId') or ''
    text_fn(f'  {time}  {sender}')
    if preview:
        text_fn(f'    {preview[:200]}')
    if thread_id:
        text_fn(f'    thread: {thread_id}')


def format_person(hit, text_fn):
    src = hit.get('Source') or {}
    name = src.get('DisplayName') or 'unknown'
    emails = src.get('EmailAddresses') or []
    email = emails[0] if emails else ''
    title = src.get('JobTitle') or ''
    dept = src.get('Department') or ''
    text_fn(f'  {name}{f" ({email})" if email else ""}{f" — {title}" if title else ""}{f", {dept}" if dept else ""}')


def format_chat(hit, text_fn):
    src = hit.get('Source') or {}
    topic = src.get('Name') or src.get('Topic') or src.get('DisplayName') or '(no topic)'
    chat_type = src.get('ThreadType') or src.get('ChatType') or ''
    member_count = src.get('TotalChatMembersCount') or ''
    last_msg = fmt_time(src.get('LastMessageTime'))
    members = ', '.join(m.get('DisplayName') for m in (src.get('ChatMembers') or []) if m.get('DisplayName'))
    text_fn(f'  [{chat_type}] {topic}{f" ({member_count} members)" if member_count else ""}{f"  {last_msg}" if last_msg else ""}')
    if members:
        text_fn(f'    members: {members}')
    if src.get('ThreadId'):
        text_fn(f'    thread: {src["ThreadId"]}')
    if src.get('Id'):
        text_fn(f'    id: {src["Id"]}')


def format_channel(hit, text_fn):
    src = hit.get('Source') or {}
    name = src.get('DisplayName') or src.get('Name') or '(unnamed)'
    team = src.get('TeamName') or ''
    summary = strip_html(hit.get('HitHighlightedSummary') or '')
    text_fn(f'  {name}{f" in {team}" if team else ""}')
    if summary:
        text_fn(f'    {summary[:200]}')


def format_file(hit, text_fn):
    src = hit.get('Source') or {}
    name = src.get('FileName') or src.get('Title') or '(untitled)'
    modified = fmt_time(src.get('LastModifiedTime'))
    modified_by = src.get('ModifiedBy') or ''
    path = src.get('Path') or ''
    text_fn(f'  {name}{f" ({modified})" if modified else ""}{f" by {modified_by}" if modified_by else ""}')
    if path:
        text_fn(f'    {path}')


def format_event(hit, text_fn):
    src = hit.get('Source') or {}
    subject = src.get('Subject') or '(no subject)'
    start = fmt_time(src.get('Start'))
    end = fmt_time(src.get('End'))
    organizer = src.get('Organizer') or ''
    text_fn(f'  {subject}')
    if start:
        text_fn(f'    {start}{f" - {end}" if end else ""}{f" ({organizer})" if organizer else ""}')


FORMATTERS = {
    'Message': format_message,
    'People': format_person,
    'Chat': format_chat,
    'TeamsChannel': format_channel,
    'File': format_file,
    'Event': format_event,
}


@click.group('search')
def search_cmd():
    """Teams / M365 search (Substrate Search API)."""


@search_cmd.command('query')
@click.argument('query', required=False, default='')
@click.option('-t', '--type', 'entity_types', default='messages', help=f'Entity types (comma-separated: {",".join(VALID_TYPES)})')
@click.option('-n', '--size', default='10')
@click.option('--from', 'from_', default='0')
@click.option('--chat', default=None)
@click.option('--sender', default=None)
@click.option('--mentions-me', is_flag=True)
@click.option('--since', default=None)
@click.option('--until', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def search_query(ctx, query, entity_types, size, from_, chat, sender, mentions_me, since, until, as_json):
    """Search across Teams messages, chats, people, files, channels, events."""
    try:
        types = [t.strip().lower() for t in entity_types.split(',')]
        for t in types:
            if t not in VALID_TYPES:
                return ctx.obj['die'](f'Invalid type "{t}". Valid: {", ".join(VALID_TYPES)}')

        filters = {}
        if chat:
            filters['chat'] = chat
        if since:
            filters['since'] = to_iso_date(since)
        if until:
            filters['until'] = to_iso_date(until)
        if sender:
            filters['sender'] = resolve_person(sender)
        if mentions_me:
            filters['mentions_me'] = True

        result = substrate_search(query, types, int(size), int(from_), filters)

        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result})

        for es in (result.get('EntitySets') or []):
            entity_type = es.get('EntityType') or 'Unknown'
            for rs in (es.get('ResultSets') or []):
                results = rs.get('Results') or []
                if not results:
                    ctx.obj['text'](f'\n--- {entity_type}: no results ---')
                    continue
                ctx.obj['text'](f'\n--- {entity_type} ({rs.get("Total") or len(results)} total, showing {len(results)}) ---')
                fmt = FORMATTERS.get(entity_type)
                for hit in results:
                    if fmt:
                        fmt(hit, ctx.obj['text'])
                    else:
                        ctx.obj['text'](f'  {str(hit.get("Source") or hit)[:200]}')
                    ctx.obj['text']('')

        for aes in (result.get('AnswerEntitySets') or []):
            entity_type = aes.get('EntityType') or 'Unknown'
            for rs in (aes.get('ResultSets') or []):
                results = rs.get('Results') or []
                if not results:
                    ctx.obj['text'](f'\n--- {entity_type}: no results ---')
                    continue
                ctx.obj['text'](f'\n--- {entity_type} ({len(results)} results) ---')
                fmt = FORMATTERS.get(entity_type)
                for hit in results:
                    if fmt:
                        fmt(hit, ctx.obj['text'])
                    else:
                        ctx.obj['text'](f'  {str(hit.get("Source") or hit)[:200]}')
                    ctx.obj['text']('')

        qa = result.get('QueryAlterations')
        if qa and qa.get('Alterations'):
            alts = [a.get('AlteredQuery') or a.get('SuggestedQuery') for a in qa['Alterations']]
            ctx.obj['text'](f'\nDid you mean: {", ".join(a for a in alts if a)}')
    except Exception as e:
        ctx.obj['die'](str(e))
