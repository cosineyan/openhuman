import os
import re
import subprocess
import urllib.parse
from datetime import datetime
import click
from ..api import graph_request


def fmt_size(b):
    if b is None:
        return ''
    if b < 1024:
        return f'{b} B'
    if b < 1024 * 1024:
        return f'{b / 1024:.1f} KB'
    if b < 1024 * 1024 * 1024:
        return f'{b / (1024 * 1024):.1f} MB'
    return f'{b / (1024 * 1024 * 1024):.1f} GB'


def fmt_time(iso):
    if not iso:
        return ''
    try:
        d = datetime.fromisoformat(iso.replace('Z', '+00:00'))
        return d.strftime('%b %-d %Y, %H:%M')
    except Exception:
        return iso[:16]


def resolve_item(item_id):
    search_result = graph_request('search/query', method='POST', body={
        'requests': [{'entityTypes': ['driveItem'], 'query': {'queryString': item_id}, 'from': 0, 'size': 5}]
    }, raw=True)
    if search_result['status_code'] != 200:
        raise RuntimeError(f'Graph Search error: {search_result["status_code"]} {search_result["body"]}')
    hits = (((search_result['body'] or {}).get('value') or [{}])[0].get('hitsContainers') or [{}])[0].get('hits') or []
    hit = next((h for h in hits if (h.get('resource') or {}).get('id') == item_id), None)
    if not hit:
        raise RuntimeError(f'Item not found: {item_id}')
    drive_id = (hit.get('resource') or {}).get('parentReference', {}).get('driveId')
    if not drive_id:
        raise RuntimeError(f'Cannot determine drive for item: {item_id}')
    result = graph_request(f'drives/{drive_id}/items/{item_id}', raw=True)
    if result['status_code'] != 200:
        raise RuntimeError(f'Graph API error: {result["status_code"]} {result["body"]}')
    return result['body']


def format_item(item, text_fn):
    is_folder = bool(item.get('folder'))
    tag = '[D]' if is_folder else '[F]'
    name = (item['name'] + '/') if is_folder else item['name']
    size = '' if is_folder else fmt_size(item.get('size'))
    time = fmt_time(item.get('lastModifiedDateTime'))
    text_fn(f'  {tag} {name:<40} {size:>10}  {time}   id: {item.get("id")}')


def format_item_detail(item, text_fn):
    is_folder = bool(item.get('folder'))
    text_fn(item.get('name'))
    if is_folder:
        text_fn(f'  Type: folder ({item["folder"].get("childCount")} items)')
    else:
        text_fn(f'  Size: {fmt_size(item.get("size"))}')
        ext = os.path.splitext(item.get('name', ''))[1].lstrip('.')
        if ext:
            text_fn(f'  Type: {ext}')
    if item.get('createdDateTime'):
        by = ((item.get('createdBy') or {}).get('user') or {}).get('displayName') or ''
        text_fn(f'  Created: {fmt_time(item["createdDateTime"])}{f" by {by}" if by else ""}')
    if item.get('lastModifiedDateTime'):
        by = ((item.get('lastModifiedBy') or {}).get('user') or {}).get('displayName') or ''
        text_fn(f'  Modified: {fmt_time(item["lastModifiedDateTime"])}{f" by {by}" if by else ""}')
    if (item.get('parentReference') or {}).get('path'):
        text_fn(f'  Parent: {item["parentReference"]["path"]}')
    dl_url = item.get('@microsoft.graph.downloadUrl')
    if dl_url:
        text_fn(f'  Download: {dl_url}')
    if item.get('webUrl'):
        text_fn(f'  Web: {item["webUrl"]}')
    text_fn(f'  ID: {item.get("id")}')


def format_activity(a, text_fn):
    action_key = list((a.get('action') or {}).keys())[0] if a.get('action') else 'unknown'
    actor_user = (a.get('actor') or {}).get('user') or {}
    actor = actor_user.get('displayName') or actor_user.get('email') or 'unknown'
    time = fmt_time(((a.get('times') or {}).get('recordedDateTime')))
    di = a.get('driveItem') or {}
    file_name = di.get('name') or ''

    detail = ''
    if action_key == 'share':
        recipients = (a['action']['share'].get('recipients') or [])
        names = [((r.get('user') or {}).get('displayName') or (r.get('user') or {}).get('email') or '?') for r in recipients]
        if len(names) <= 3:
            detail = f'→ {", ".join(names)}'
        else:
            detail = f'→ {", ".join(names[:3])} (+{len(names) - 3} more)'
    elif action_key == 'rename':
        old_name = a['action']['rename'].get('oldName') or ''
        if old_name:
            detail = f'from "{old_name}"'
    elif action_key == 'version':
        ver = a['action']['version'].get('newVersion') or ''
        if ver:
            detail = f'v{ver}'
    elif action_key == 'mention':
        mentionees = a['action']['mention'].get('mentionees') or []
        names = [(m.get('user') or {}).get('displayName') or '?' for m in mentionees]
        if names:
            detail = f'@{", @".join(names)}'

    text_fn(f'  {time}  {action_key}  {actor}')
    if file_name:
        text_fn(f'    {file_name}')
    if detail:
        text_fn(f'    {detail}')


def resolve_sp_drive(sp_path):
    site_result = graph_request(f'sites/sap.sharepoint.com:/{sp_path}', raw=True)
    if site_result['status_code'] != 200:
        raise RuntimeError(f'Cannot resolve SharePoint site "{sp_path}": {site_result["status_code"]} {site_result["body"]}')
    site_id = site_result['body']['id']
    drives_result = graph_request(f'sites/{site_id}/drives?$select=id,name,webUrl', raw=True)
    if drives_result['status_code'] != 200:
        raise RuntimeError(f'Cannot list drives for site "{sp_path}": {drives_result["status_code"]}')
    drives = (drives_result['body'] or {}).get('value') or []
    if not drives:
        raise RuntimeError(f'No document libraries found for site "{sp_path}"')
    main_drive = next((d for d in drives if re.match(r'^(Shared )?Documents$', d.get('name') or '', re.IGNORECASE)), drives[0])
    return main_drive['id']


@click.group('files')
def files_cmd():
    """OneDrive files (Graph API)."""


@files_cmd.command('list')
@click.argument('folder_path', required=False)
@click.option('-n', '--top', default='500')
@click.option('-s', '--sort', default='name', help='Sort by: name, modified, size, created (prefix with - for desc)')
@click.option('--sp', default=None)
@click.option('--skip', 'skip_token', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def files_list(ctx, folder_path, top, sort, sp, skip_token, as_json):
    """List folder contents."""
    try:
        sort_fields = {'name': 'name', 'modified': 'lastModifiedDateTime', 'size': 'size', 'created': 'createdDateTime'}
        sort_raw = sort or 'name'
        sort_dir = 'asc'
        if sort_raw.startswith('-'):
            sort_dir = 'desc'
            sort_raw = sort_raw[1:]
        sort_field = sort_fields.get(sort_raw)
        if not sort_field:
            return ctx.obj['die'](f'Invalid sort field "{sort_raw}". Valid: {", ".join(sort_fields)}')
        orderby = f'{sort_field} {sort_dir}'

        drive_prefix = 'drive'
        raw = False
        display_label = 'OneDrive'
        if sp:
            drive_id = resolve_sp_drive(sp)
            drive_prefix = f'drives/{drive_id}'
            raw = True
            display_label = sp

        if folder_path:
            p = folder_path.strip('/')
            endpoint = f'{drive_prefix}/root:/{p}:/children?$top={int(top)}&$orderby={urllib.parse.quote(orderby)}'
        else:
            endpoint = f'{drive_prefix}/root/children?$top={int(top)}&$orderby={urllib.parse.quote(orderby)}'
        if skip_token:
            endpoint += f'&$skiptoken={skip_token}'

        result = graph_request(endpoint, raw=raw)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        items = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        skip_tok = ''
        if next_link:
            m = re.search(r'\$skiptoken=([^&]+)', next_link)
            if m:
                skip_tok = m.group(1)

        if as_json:
            return ctx.obj['out']({'ok': True, 'data': items, 'nextSkip': skip_tok or None})

        display_path = folder_path or '/'
        if not items:
            return ctx.obj['text'](f'--- {display_label}: {display_path} (empty) ---')
        ctx.obj['text'](f'--- {display_label}: {display_path} ({len(items)} items) ---\n')
        for item in items:
            format_item(item, ctx.obj['text'])
        if skip_tok:
            ctx.obj['text'](f'\nMore results available. Use: --skip {skip_tok}')
    except Exception as e:
        ctx.obj['die'](str(e))


@files_cmd.command('search')
@click.argument('query')
@click.option('-n', '--top', default='50')
@click.option('--from', 'from_', default='0')
@click.option('-a', '--all', 'search_all', is_flag=True)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def files_search(ctx, query, top, from_, search_all, as_json):
    """Search files in OneDrive."""
    try:
        top_n = min(int(top), 500)
        from_n = int(from_)

        if search_all:
            body = {'requests': [{'entityTypes': ['driveItem'], 'query': {'queryString': query}, 'from': from_n, 'size': top_n}]}
            result = graph_request('search/query', method='POST', body=body, raw=True)
            if result['status_code'] != 200:
                return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
            container = (((result['body'] or {}).get('value') or [{}])[0].get('hitsContainers') or [{}])[0]
            hits = container.get('hits') or []
            total = container.get('total') or len(hits)
            more = container.get('moreResultsAvailable') or False
            if as_json:
                return ctx.obj['out']({'ok': True, 'data': hits, 'total': total, 'from': from_n, 'moreAvailable': more})
            if not hits:
                return ctx.obj['text']('No files found.')
            ctx.obj['text'](f'--- Search (all): "{query}" (showing {from_n + 1}-{from_n + len(hits)} of {total}) ---\n')
            for hit in hits:
                format_item(hit.get('resource') or {}, ctx.obj['text'])
            if more:
                ctx.obj['text'](f'\nMore results available. Use: --from {from_n + len(hits)}')
            return

        endpoint = f"drive/search(q='{urllib.parse.quote(query)}')?$top={top_n}"
        if from_n > 0:
            endpoint += f'&$skip={from_n}'
        result = graph_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        items = (result['body'] or {}).get('value') or []
        has_next = bool((result['body'] or {}).get('@odata.nextLink'))
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': items, 'from': from_n, 'hasNext': has_next})
        if not items:
            return ctx.obj['text']('No files found.')
        ctx.obj['text'](f'--- Search: "{query}" ({len(items)} results, from {from_n}) ---\n')
        for item in items:
            format_item(item, ctx.obj['text'])
        if has_next:
            ctx.obj['text'](f'\nMore results available. Use: --from {from_n + len(items)}')
    except Exception as e:
        ctx.obj['die'](str(e))


@files_cmd.command('shared')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def files_shared(ctx, as_json):
    """List files shared with me."""
    try:
        result = graph_request('drive/sharedWithMe')
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        items = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': items})
        if not items:
            return ctx.obj['text']('--- Shared with me (empty) ---')
        ctx.obj['text'](f'--- Shared with me ({len(items)} items) ---\n')
        for item in items:
            format_item(item, ctx.obj['text'])
    except Exception as e:
        ctx.obj['die'](str(e))


@files_cmd.command('get')
@click.argument('item_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def files_get(ctx, item_id, as_json):
    """Get file/folder metadata."""
    try:
        item = resolve_item(item_id)
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': item})
        format_item_detail(item, ctx.obj['text'])
    except Exception as e:
        ctx.obj['die'](str(e))


@files_cmd.command('activities')
@click.argument('item_id', required=False)
@click.option('-n', '--top', default='20')
@click.option('--sp', default=None)
@click.option('--skip', 'skip_token', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def files_activities(ctx, item_id, top, sp, skip_token, as_json):
    """List recent activities."""
    try:
        raw = False
        if item_id:
            item = resolve_item(item_id)
            drive_id = (item.get('parentReference') or {}).get('driveId')
            if not drive_id:
                return ctx.obj['die']('Cannot determine drive for this item')
            endpoint = f'drives/{drive_id}/items/{item_id}/activities?$top={int(top)}&$expand=driveItem'
            raw = True
        elif sp:
            drive_id = resolve_sp_drive(sp)
            endpoint = f'drives/{drive_id}/activities?$top={int(top)}&$expand=driveItem'
            raw = True
        else:
            endpoint = f'drive/activities?$top={int(top)}&$expand=driveItem'

        if skip_token:
            endpoint += f'&$skiptoken={skip_token}'

        result = graph_request(endpoint, raw=raw)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        activities = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        skip_tok = ''
        if next_link:
            m = re.search(r'skiptoken=([^&]+)', next_link)
            if m:
                skip_tok = m.group(1)

        if as_json:
            return ctx.obj['out']({'ok': True, 'data': activities, 'nextSkip': skip_tok or None})
        if not activities:
            return ctx.obj['text']('No recent activities.')
        label = f'Item {item_id}' if item_id else (sp or 'OneDrive')
        ctx.obj['text'](f'--- Activities: {label} ({len(activities)}) ---\n')
        for a in activities:
            format_activity(a, ctx.obj['text'])
            ctx.obj['text']('')
        if skip_tok:
            ctx.obj['text'](f'More results available. Use: --skip {skip_tok}')
    except Exception as e:
        ctx.obj['die'](str(e))


@files_cmd.command('open')
@click.argument('item_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def files_open(ctx, item_id, as_json):
    """Open file in browser."""
    try:
        item = resolve_item(item_id)
        web_url = item.get('webUrl')
        if not web_url:
            return ctx.obj['die']('No web URL available for this item')
        if as_json:
            return ctx.obj['out']({'ok': True, 'webUrl': web_url})
        subprocess.run(['open', web_url], check=True)
        ctx.obj['text'](f'Opened: {web_url}')
    except Exception as e:
        ctx.obj['die'](str(e))
