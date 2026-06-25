import base64
import json as _json
import urllib.request
import urllib.error
from datetime import datetime, timezone
import click
from ..tokens import ensure_substrate_token, ensure_token

LOOP_API_BASE = 'https://substrate.office.com/recommended/api/v1.1/loop'


def get_user_context():
    graph_token = ensure_token('graph')
    padded = graph_token.split('.')[1]
    padded += '=' * (-len(padded) % 4)
    payload = _json.loads(base64.b64decode(padded).decode())
    return {'puid': payload.get('puid'), 'tenant_id': payload.get('tid')}


def loop_headers(scenario=None):
    token = ensure_substrate_token()
    ctx = get_user_context()
    return {
        'Authorization': f'Bearer {token}',
        'X-Office-Application': '300',
        'X-Office-AppScenario': scenario or 'Workspaces',
        'X-Office-AudienceGroup': 'Production',
        'X-Office-Platform': 'Web',
        'X-Office-Version': '20260319100',
        'X-AnchorMailbox': f'PUID:{ctx["puid"]}@{ctx["tenant_id"]}',
        'Origin': 'https://loop.cloud.microsoft',
        'Referer': 'https://loop.cloud.microsoft/',
    }


def loop_request(path, scenario=None):
    url = f'{LOOP_API_BASE}/{path}'
    headers = loop_headers(scenario)
    req = urllib.request.Request(url, headers=headers)
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return _json.loads(resp.read().decode())
    except urllib.error.HTTPError as e:
        body = e.read().decode()
        raise RuntimeError(f'Loop API {e.code}: {body[:200]}')


def relative_time(iso_date):
    if not iso_date:
        return ''
    try:
        d = datetime.fromisoformat(iso_date.replace('Z', '+00:00'))
        diff_ms = (datetime.now(timezone.utc) - d).total_seconds() * 1000
        mins = int(diff_ms / 60000)
        if mins < 1:
            return 'just now'
        if mins < 60:
            return f'{mins}m ago'
        hours = mins // 60
        if hours < 24:
            return f'{hours}h ago'
        days = hours // 24
        if days < 7:
            return f'{days}d ago'
        return d.strftime('%b %-d')
    except Exception:
        return iso_date[:10]


def format_page(p, text_fn):
    title = p.get('title') or '(untitled)'
    ext = f'.{p["extension"]}' if p.get('extension') else ''
    when = relative_time((p.get('user_relationship') or {}).get('last_access_datetime') or p.get('last_store_modified_datetime'))
    badge = (p.get('activity_badge') or {}).get('message_format') or ''
    container = (p.get('container_info') or {}).get('title') or ''
    url = p.get('web_url') or ''
    text_fn(f'  {title}')
    meta = ' | '.join(x for x in [ext, when, container] if x)
    if meta:
        text_fn(f'    {meta}')
    if badge:
        user_name = ((p.get('activity_badge') or {}).get('users') or [{}])[0].get('display_name') or ''
        text_fn(f'    {badge.replace("{0}", user_name)}')
    if url:
        text_fn(f'    {url}')


@click.group('loop')
def loop_cmd():
    """Microsoft Loop (pages, meeting notes, search)."""


@loop_cmd.command('recent')
@click.option('-n', '--top', default='20')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def loop_recent(ctx, top, as_json):
    """List recently accessed Loop pages and components."""
    try:
        data = loop_request(f'recent?top={int(top)}&settings=true&rs=en-us')
        pages = data.get('pages') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': pages})
        if not pages:
            return ctx.obj['text']('No recent Loop pages.')
        ctx.obj['text'](f'--- Recent Loop pages ({len(pages)}) ---\n')
        for p in pages:
            format_page(p, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@loop_cmd.command('meeting-notes')
@click.option('-n', '--top', default='30')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def loop_meeting_notes(ctx, top, as_json):
    """List recent meeting notes from Loop."""
    try:
        data = loop_request(f'recent?top={int(top)}&settings=true&rs=en-us')
        pages = [p for p in (data.get('pages') or []) if
                 (p.get('container_info') or {}).get('title', '').lower() == 'meetings' or
                 'meetings/' in (p.get('container_info') or {}).get('url', '').lower() or
                 'meeting ' in (p.get('container_info') or {}).get('url', '').lower()]
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': pages})
        if not pages:
            return ctx.obj['text']('No meeting notes found.')
        ctx.obj['text'](f'--- Meeting notes ({len(pages)}) ---\n')
        for p in pages:
            format_page(p, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@loop_cmd.command('search')
@click.argument('query')
@click.option('-n', '--top', default='20')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def loop_search(ctx, query, top, as_json):
    """Search Loop pages and components."""
    try:
        data = loop_request('recent?top=30&settings=true&rs=en-us')
        all_pages = data.get('pages') or []
        q = query.lower()
        matched = [p for p in all_pages if q in (p.get('title') or '').lower() or q in (p.get('container_info') or {}).get('title', '').lower()]
        pages = matched[:int(top)]
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': pages})
        if not pages:
            return ctx.obj['text'](f'No Loop pages matching "{query}".')
        ctx.obj['text'](f'--- Loop search: "{query}" ({len(pages)} results) ---\n')
        for p in pages:
            format_page(p, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))
