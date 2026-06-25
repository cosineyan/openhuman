import os
import re
import base64
import urllib.parse
from datetime import datetime
import click
from ..api import rest_request

MIME_TYPES = {
    '.pdf': 'application/pdf',
    '.doc': 'application/msword',
    '.docx': 'application/vnd.openxmlformats-officedocument.wordprocessingml.document',
    '.xls': 'application/vnd.ms-excel',
    '.xlsx': 'application/vnd.openxmlformats-officedocument.spreadsheetml.sheet',
    '.ppt': 'application/vnd.ms-powerpoint',
    '.pptx': 'application/vnd.openxmlformats-officedocument.presentationml.presentation',
    '.txt': 'text/plain',
    '.csv': 'text/csv',
    '.html': 'text/html',
    '.htm': 'text/html',
    '.json': 'application/json',
    '.xml': 'application/xml',
    '.zip': 'application/zip',
    '.png': 'image/png',
    '.jpg': 'image/jpeg',
    '.jpeg': 'image/jpeg',
    '.gif': 'image/gif',
    '.svg': 'image/svg+xml',
    '.mp4': 'video/mp4',
    '.mp3': 'audio/mpeg',
}
MAX_ATTACHMENT_SIZE = 4 * 1024 * 1024


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


def fmt_size(b):
    if b is None:
        return ''
    if b < 1024:
        return f'{b} B'
    if b < 1024 * 1024:
        return f'{b / 1024:.1f} KB'
    return f'{b / (1024 * 1024):.1f} MB'


def parse_recipients(s):
    if not s:
        return []
    return [{'EmailAddress': {'Address': addr.strip()}} for addr in s.split(',') if addr.strip()]


def fmt_recipients(arr):
    if not arr:
        return ''
    parts = []
    for r in arr:
        ea = (r.get('EmailAddress') or {})
        name = ea.get('Name') or ''
        addr = ea.get('Address') or ''
        parts.append(f'{name} <{addr}>' if name and name != addr else addr)
    return ', '.join(parts)


def add_attachments(draft_id, file_paths):
    results = []
    for file_path in file_paths:
        full_path = os.path.realpath(file_path)
        if not os.path.exists(full_path):
            raise RuntimeError(f'Attachment not found: {file_path}')
        size = os.path.getsize(full_path)
        if size > MAX_ATTACHMENT_SIZE:
            raise RuntimeError(f'Attachment too large ({size / (1024*1024):.1f} MB): {file_path}. Max 4 MB.')
        with open(full_path, 'rb') as f:
            content_bytes = base64.b64encode(f.read()).decode()
        ext = os.path.splitext(full_path)[1].lower()
        file_name = os.path.basename(full_path)
        result = rest_request(f'messages/{draft_id}/attachments', method='POST', body={
            '@odata.type': '#Microsoft.OutlookServices.FileAttachment',
            'Name': file_name,
            'ContentType': MIME_TYPES.get(ext, 'application/octet-stream'),
            'ContentBytes': content_bytes,
        })
        if result['status_code'] != 201:
            raise RuntimeError(f'Failed to attach "{file_name}": {result["status_code"]} {result["body"]}')
        results.append({'name': file_name, 'size': size})
    return results


WELL_KNOWN_FOLDERS = ['inbox', 'drafts', 'sentitems', 'deleteditems', 'junkemail', 'archive']


def resolve_folder(folder):
    lower = folder.lower()
    if lower in WELL_KNOWN_FOLDERS:
        return lower
    if len(folder) > 30:
        return folder

    result = rest_request('mailfolders?$top=100&$select=DisplayName,Id,ChildFolderCount')
    if result['status_code'] != 200:
        raise RuntimeError(f'Cannot list folders: {result["status_code"]}')
    folders = (result['body'] or {}).get('value') or []
    match = next((f for f in folders if (f.get('DisplayName') or '').lower() == lower), None)
    if match:
        return match['Id']

    for f in folders:
        if not f.get('ChildFolderCount'):
            continue
        child_result = rest_request(f'mailfolders/{urllib.parse.quote(f["Id"])}/childfolders?$top=100&$select=DisplayName,Id')
        if child_result['status_code'] != 200:
            continue
        child_match = next((c for c in (child_result['body'] or {}).get('value') or []
                            if (c.get('DisplayName') or '').lower() == lower), None)
        if child_match:
            return child_match['Id']

    raise RuntimeError(f'Folder not found: "{folder}"')


def format_email(m, idx, text_fn):
    fr = (m.get('From') or {}).get('EmailAddress') or {}
    sender_name = fr.get('Name') or ''
    sender_addr = fr.get('Address') or ''
    sender = f'{sender_name} <{sender_addr}>' if sender_name and sender_name != sender_addr else (sender_addr or 'unknown')
    time = fmt_time(m.get('ReceivedDateTime'))
    read = 'Read  ' if m.get('IsRead') else 'Unread'
    imp = ' [HIGH]' if m.get('Importance') == 'High' else ''
    attach = ' [attach]' if m.get('HasAttachments') else ''
    is_meeting = 'EventMessage' in (m.get('@odata.type') or '')
    mtg = ' [meeting]' if is_meeting else ''
    subj = m.get('Subject') or '(no subject)'
    preview = re.sub(r'\r?\n', ' ', m.get('BodyPreview') or '')[:150].strip()

    text_fn(f'{str(idx).rjust(3)}.  [{time}]  {read}  {sender}{imp}{attach}{mtg}')
    text_fn(f'      {subj}')
    if is_meeting:
        start = m.get('StartDateTime', {}).get('DateTime')
        end = m.get('EndDateTime', {}).get('DateTime')
        parts = []
        if start:
            parts.append(fmt_time(start + 'Z'))
        if end:
            parts.append(fmt_time(end + 'Z'))
        loc = (m.get('Location') or {}).get('DisplayName')
        if loc:
            parts.append(loc)
        if m.get('IsAllDay'):
            parts.append('All Day')
        if parts:
            text_fn(f'      Meeting: {" - ".join(parts)}')
    if preview:
        text_fn(f'      {preview}')
    text_fn(f'      id: {m.get("Id")}')


@click.group('mail')
def mail_cmd():
    """Outlook email (REST API v2.0)."""


@mail_cmd.command('folders')
@click.option('-n', '--top', default='100')
@click.option('--skip', default='0')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_folders(ctx, top, skip, as_json):
    """List mail folders."""
    try:
        result = rest_request(f'mailfolders?$top={int(top)}&$skip={int(skip)}&$select=DisplayName,Id,TotalItemCount,UnreadItemCount,ChildFolderCount')
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        folders = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': folders, 'count': len(folders), 'nextSkip': int(skip) + int(top) if next_link else None})
        if not folders:
            return ctx.obj['text']('No mail folders found.')
        ctx.obj['text']('--- Mail Folders ---\n')
        for f in folders:
            name = (f.get('DisplayName') or '(unnamed)').ljust(30)
            total = str(f.get('TotalItemCount') or 0).rjust(6)
            unread = f', {f["UnreadItemCount"]} unread' if f.get('UnreadItemCount') else ''
            children = f'  ({f["ChildFolderCount"]} child)' if f.get('ChildFolderCount') else ''
            ctx.obj['text'](f'  {name} {total} total{unread}{children}')
        if next_link:
            ctx.obj['text'](f'\nMore results available. Use: --skip {int(skip) + int(top)}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('list')
@click.argument('folder', required=False, default='inbox')
@click.option('-n', '--top', default='20')
@click.option('--skip', default='0')
@click.option('--unread', is_flag=True)
@click.option('--important', is_flag=True)
@click.option('--has-attachments', is_flag=True)
@click.option('--since', default=None)
@click.option('--until', default=None)
@click.option('--from', 'from_addr', default=None)
@click.option('--exclude', 'exclude_folders', multiple=True, help='Exclude folder(s) when using "all"')
@click.option('-s', '--select', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_list(ctx, folder, top, skip, unread, important, has_attachments, since, until, from_addr, exclude_folders, select, as_json):
    """List emails from a folder (default: inbox, use "all" for all folders)."""
    try:
        is_all = folder == 'all'
        top_i = int(top)
        skip_i = int(skip)

        filters = []
        if unread:
            filters.append('IsRead eq false')
        if important:
            filters.append("Importance eq 'High'")
        if has_attachments:
            filters.append('HasAttachments eq true')
        if since:
            filters.append(f'ReceivedDateTime ge {datetime.fromisoformat(since.replace("Z", "+00:00")).isoformat()}')
        if until:
            filters.append(f'ReceivedDateTime lt {datetime.fromisoformat(until.replace("Z", "+00:00")).isoformat()}')
        if from_addr:
            filters.append(f"From/EmailAddress/Address eq '{from_addr}'")

        if is_all and exclude_folders:
            for ex in exclude_folders:
                folder_id = resolve_folder(ex)
                if folder_id in WELL_KNOWN_FOLDERS:
                    r = rest_request(f'mailfolders/{folder_id}?$select=Id')
                    if r['status_code'] == 200 and (r['body'] or {}).get('Id'):
                        filters.append(f"ParentFolderId ne '{r['body']['Id']}'")
                else:
                    filters.append(f"ParentFolderId ne '{folder_id}'")

        unsortable = important or has_attachments or from_addr
        params = [f'$top={top_i}', f'$skip={skip_i}']
        if not unsortable:
            params.append('$orderby=ReceivedDateTime desc')
        if select:
            params.append(f'$select={select}')
        if filters:
            params.append(f'$filter={" and ".join(filters)}')

        if is_all:
            endpoint = f'messages?{"&".join(params)}'
        else:
            resolved_folder = resolve_folder(folder)
            endpoint = f'mailfolders/{urllib.parse.quote(str(resolved_folder))}/messages?{"&".join(params)}'

        result = rest_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        messages = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': messages, 'count': len(messages),
                                   'nextSkip': skip_i + top_i if next_link else None})
        label = 'all folders' if is_all else folder
        if not messages:
            return ctx.obj['text'](f'--- {label} (empty) ---')
        ctx.obj['text'](f'--- {label} ({len(messages)} emails) ---\n')
        for i, m in enumerate(messages):
            format_email(m, skip_i + i + 1, ctx.obj['text'])
            ctx.obj['text']('')
        if next_link:
            ctx.obj['text'](f'More results available. Use: --skip {skip_i + top_i}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('read')
@click.argument('msg_id')
@click.option('--html', 'show_html', is_flag=True)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_read(ctx, msg_id, show_html, as_json):
    """Read a specific email (full body)."""
    try:
        result = rest_request(f'messages/{msg_id}')
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        m = result['body']
        is_meeting = 'EventMessage' in (m.get('@odata.type') or '')
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': m})
        ctx.obj['text'](f'Subject: {m.get("Subject") or "(no subject)"}')
        fr = (m.get('From') or {}).get('EmailAddress') or {}
        fr_name = fr.get('Name') or ''
        fr_addr = fr.get('Address') or ''
        ctx.obj['text'](f'From: {f"{fr_name} <{fr_addr}>" if fr_name else (fr_addr or "unknown")}')
        to = fmt_recipients(m.get('ToRecipients'))
        if to:
            ctx.obj['text'](f'To: {to}')
        cc = fmt_recipients(m.get('CcRecipients'))
        if cc:
            ctx.obj['text'](f'Cc: {cc}')
        bcc = fmt_recipients(m.get('BccRecipients'))
        if bcc:
            ctx.obj['text'](f'Bcc: {bcc}')
        ctx.obj['text'](f'Date: {fmt_time(m.get("ReceivedDateTime"))}')
        if m.get('Importance') and m['Importance'] != 'Normal':
            ctx.obj['text'](f'Importance: {m["Importance"]}')
        if m.get('Categories'):
            ctx.obj['text'](f'Categories: {", ".join(m["Categories"])}')
        if is_meeting:
            ctx.obj['text']('')
            mtype = m.get('MeetingMessageType') or ''
            label = '(Cancelled)' if mtype == 'MeetingCancellation' else '(Request)' if mtype == 'MeetingRequest' else f'({mtype})'
            ctx.obj['text'](f'Type: Meeting {label}')
            start = (m.get('StartDateTime') or {}).get('DateTime')
            end = (m.get('EndDateTime') or {}).get('DateTime')
            if start:
                ctx.obj['text'](f'Start: {fmt_time(start + "Z")}')
            if end:
                ctx.obj['text'](f'End: {fmt_time(end + "Z")}')
            if m.get('IsAllDay'):
                ctx.obj['text']('All Day: Yes')
            loc = (m.get('Location') or {}).get('DisplayName')
            if loc:
                ctx.obj['text'](f'Location: {loc}')
        attachments = m.get('Attachments') or []
        if attachments:
            def _att_str(a):
                name = a.get('Name') or '(unnamed)'
                sz = a.get('Size')
                return f'{name} ({fmt_size(sz)})' if sz else name
            att_list = ', '.join(_att_str(a) for a in attachments)
            ctx.obj['text'](f'Attachments: {att_list}')
        ctx.obj['text']('')
        body = m.get('Body') or {}
        if show_html and body.get('ContentType') == 'HTML':
            ctx.obj['text'](body.get('Content') or '')
        else:
            ctx.obj['text'](strip_html(body.get('Content') or '') if body.get('ContentType') == 'HTML' else (body.get('Content') or ''))
        ctx.obj['text']('')
        ctx.obj['text'](f'ID: {m.get("Id")}')
        if m.get('ConversationId'):
            ctx.obj['text'](f'ConversationId: {m["ConversationId"]}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('search')
@click.argument('query')
@click.option('-n', '--top', default='20')
@click.option('--skip-token', default=None)
@click.option('--folder', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_search(ctx, query, top, skip_token, folder, as_json):
    """Search emails (KQL syntax)."""
    try:
        base = f'mailfolders/{folder}/messages' if folder else 'messages'
        import urllib.parse as _up
        if skip_token:
            endpoint = f'{base}?$search="{_up.quote(query)}"&$top={int(top)}&$skiptoken={skip_token}'
        else:
            endpoint = f'{base}?$search="{_up.quote(query)}"&$top={int(top)}'
        result = rest_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        messages = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        skip_tok = ''
        if next_link:
            m = re.search(r'\$skiptoken=([^&]+)', next_link)
            if m:
                skip_tok = m.group(1)
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': messages, 'count': len(messages), 'nextSkipToken': skip_tok or None})
        if not messages:
            return ctx.obj['text'](f'No results for "{query}".')
        ctx.obj['text'](f'--- Search: "{query}" ({len(messages)} results) ---\n')
        for i, m in enumerate(messages):
            format_email(m, i + 1, ctx.obj['text'])
            ctx.obj['text']('')
        if skip_tok:
            ctx.obj['text'](f'More results available. Use: --skip-token {skip_tok}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('draft')
@click.option('--subject', required=True)
@click.option('--to', default=None)
@click.option('--cc', default=None)
@click.option('--bcc', default=None)
@click.option('--body', 'body_text', default=None)
@click.option('--body-file', default=None)
@click.option('--html', is_flag=True)
@click.option('--importance', default='Normal')
@click.option('--attachment', 'attachments', multiple=True)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_draft(ctx, subject, to, cc, bcc, body_text, body_file, html, importance, attachments, as_json):
    """Create a draft email."""
    try:
        message = {'Subject': subject, 'Importance': importance}
        if to:
            message['ToRecipients'] = parse_recipients(to)
        if cc:
            message['CcRecipients'] = parse_recipients(cc)
        if bcc:
            message['BccRecipients'] = parse_recipients(bcc)
        content = body_text or ''
        if body_file:
            with open(body_file) as f:
                content = f.read()
        if content:
            message['Body'] = {'ContentType': 'HTML' if html else 'Text', 'Content': content}
        result = rest_request('messages', method='POST', body=message)
        if result['status_code'] != 201:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        draft_id = (result['body'] or {}).get('Id')
        attached = add_attachments(draft_id, attachments) if attachments and draft_id else []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body'], 'attachments': attached or None})
        ctx.obj['text'](f'Draft created: {(result["body"] or {}).get("Subject") or "(no subject)"}')
        ctx.obj['text'](f'  id: {draft_id}')
        if attached:
            ctx.obj['text'](f'  attachments: {", ".join(a["name"] for a in attached)}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('update')
@click.argument('msg_id')
@click.option('--subject', default=None)
@click.option('--to', default=None)
@click.option('--cc', default=None)
@click.option('--bcc', default=None)
@click.option('--body', 'body_text', default=None)
@click.option('--body-file', default=None)
@click.option('--html', is_flag=True)
@click.option('--importance', default=None)
@click.option('--attachment', 'attachments', multiple=True)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_update(ctx, msg_id, subject, to, cc, bcc, body_text, body_file, html, importance, attachments, as_json):
    """Update a draft email."""
    try:
        patch = {}
        if subject:
            patch['Subject'] = subject
        if to:
            patch['ToRecipients'] = parse_recipients(to)
        if cc:
            patch['CcRecipients'] = parse_recipients(cc)
        if bcc:
            patch['BccRecipients'] = parse_recipients(bcc)
        if importance:
            patch['Importance'] = importance
        content = body_file and open(body_file).read() or body_text
        if content:
            patch['Body'] = {'ContentType': 'HTML' if html else 'Text', 'Content': content}
        if not patch and not attachments:
            return ctx.obj['die']('Nothing to update.')
        attached = []
        if patch:
            result = rest_request(f'messages/{msg_id}', method='PATCH', body=patch)
            if result['status_code'] != 200:
                return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if attachments:
            attached = add_attachments(msg_id, attachments)
        if as_json:
            return ctx.obj['out']({'ok': True, 'updated': True, 'attachments': attached or None})
        ctx.obj['text'](f'Draft updated: {msg_id}')
        if attached:
            ctx.obj['text'](f'  attachments added: {", ".join(a["name"] for a in attached)}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('send')
@click.option('--to', required=True)
@click.option('--subject', required=True)
@click.option('--body', 'body_text', default=None)
@click.option('--body-file', default=None)
@click.option('--cc', default=None)
@click.option('--bcc', default=None)
@click.option('--html', is_flag=True)
@click.option('--importance', default='Normal')
@click.option('--attachment', 'attachments', multiple=True)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_send(ctx, to, subject, body_text, body_file, cc, bcc, html, importance, attachments, as_json):
    """Send an email."""
    try:
        content = body_text or ''
        if body_file:
            with open(body_file) as f:
                content = f.read()
        if not content:
            return ctx.obj['die']('No body provided. Use --body or --body-file.')
        message = {
            'Subject': subject,
            'Body': {'ContentType': 'HTML' if html else 'Text', 'Content': content},
            'ToRecipients': parse_recipients(to),
            'Importance': importance,
        }
        if cc:
            message['CcRecipients'] = parse_recipients(cc)
        if bcc:
            message['BccRecipients'] = parse_recipients(bcc)

        if not attachments:
            result = rest_request('sendmail', method='POST', body={'Message': message, 'SaveToSentItems': True})
            if result['status_code'] != 202:
                return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
            if as_json:
                return ctx.obj['out']({'ok': True, 'sent': True})
            recipients = ', '.join(r['EmailAddress']['Address'] for r in parse_recipients(to))
            ctx.obj['text'](f'Email sent to {recipients}')
        else:
            draft_result = rest_request('messages', method='POST', body=message)
            if draft_result['status_code'] != 201:
                return ctx.obj['out']({'ok': False, 'status_code': draft_result['status_code'], 'error': draft_result['body'], 'step': 'createDraft'})
            draft_id = (draft_result['body'] or {}).get('Id')
            if not draft_id:
                return ctx.obj['die']('Failed to create draft: no draft ID returned')
            attached = add_attachments(draft_id, attachments)
            send_result = rest_request(f'messages/{draft_id}/send', method='POST')
            if send_result['status_code'] != 202:
                return ctx.obj['out']({'ok': False, 'status_code': send_result['status_code'], 'error': send_result['body'], 'step': 'send', 'draftId': draft_id})
            if as_json:
                return ctx.obj['out']({'ok': True, 'sent': True, 'attachments': attached})
            recipients = ', '.join(r['EmailAddress']['Address'] for r in parse_recipients(to))
            att_names = ', '.join(a['name'] for a in attached)
            ctx.obj['text'](f'Email sent to {recipients} with {len(attached)} attachment(s): {att_names}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('send-draft')
@click.argument('msg_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_send_draft(ctx, msg_id, as_json):
    """Send an existing draft email."""
    try:
        result = rest_request(f'messages/{msg_id}/send', method='POST')
        if result['status_code'] != 202:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'sent': True})
        ctx.obj['text']('Draft sent.')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('move')
@click.argument('msg_id')
@click.argument('folder')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_move(ctx, msg_id, folder, as_json):
    """Move an email to a folder."""
    try:
        dest_id = resolve_folder(folder)
        result = rest_request(f'messages/{msg_id}/move', method='POST', body={'DestinationId': dest_id})
        if result['status_code'] != 201:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        ctx.obj['text'](f'Moved to {folder}. New ID: {(result["body"] or {}).get("Id") or "(unknown)"}')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('reply')
@click.argument('msg_id')
@click.option('--body', 'body_text', required=True)
@click.option('--body-file', default=None)
@click.option('--html', is_flag=True)
@click.option('--all', 'reply_all', is_flag=True)
@click.option('--attachment', 'attachments', multiple=True)
@click.option('--draft', is_flag=True, help='Save as draft instead of sending')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_reply(ctx, msg_id, body_text, body_file, html, reply_all, attachments, draft, as_json):
    """Reply to an email."""
    try:
        content = body_file and open(body_file).read() or body_text
        if not content:
            return ctx.obj['die']('No body provided.')
        action = 'createReplyAll' if reply_all else 'createReply'
        draft_result = rest_request(f'messages/{msg_id}/{action}', method='POST')
        if draft_result['status_code'] != 201:
            return ctx.obj['out']({'ok': False, 'status_code': draft_result['status_code'],
                                   'error': draft_result['body'], 'step': 'createReply'})
        draft_id = (draft_result['body'] or {}).get('Id')
        if not draft_id:
            return ctx.obj['die']('Failed to create reply draft: no draft ID returned')

        original_body = (draft_result['body'] or {}).get('Body', {}).get('Content') or ''
        if html:
            full_content = content + original_body
        else:
            escaped = (content
                       .replace('&', '&amp;')
                       .replace('<', '&lt;')
                       .replace('>', '&gt;')
                       .replace('\n', '<br>'))
            full_content = f'<p>{escaped}</p>' + original_body

        patch_result = rest_request(f'messages/{draft_id}', method='PATCH',
                                    body={'Body': {'ContentType': 'HTML', 'Content': full_content}})
        if patch_result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': patch_result['status_code'],
                                   'error': patch_result['body'], 'step': 'updateDraft', 'draftId': draft_id})

        attached = add_attachments(draft_id, attachments) if attachments else []

        if draft:
            if as_json:
                return ctx.obj['out']({'ok': True, 'sent': False, 'draft': True, 'draftId': draft_id,
                                       'replyAll': bool(reply_all),
                                       'attachments': attached if attached else None})
            reply_type = 'Reply-all' if reply_all else 'Reply'
            ctx.obj['text'](f'{reply_type} draft saved. ID: {draft_id}')
            return

        send_result = rest_request(f'messages/{draft_id}/send', method='POST')
        if send_result['status_code'] != 202:
            return ctx.obj['out']({'ok': False, 'status_code': send_result['status_code'],
                                   'error': send_result['body'], 'step': 'send', 'draftId': draft_id})
        if as_json:
            return ctx.obj['out']({'ok': True, 'sent': True, 'replyAll': bool(reply_all),
                                   'attachments': attached or None})
        reply_type = 'Reply-all' if reply_all else 'Reply'
        if attached:
            ctx.obj['text'](f'{reply_type} sent with {len(attached)} attachment(s): {", ".join(a["name"] for a in attached)}')
        else:
            ctx.obj['text'](f'{reply_type} sent.')
    except Exception as e:
        ctx.obj['die'](str(e))


@mail_cmd.command('forward')
@click.argument('msg_id')
@click.option('--to', required=True)
@click.option('--body', 'body_text', default=None)
@click.option('--body-file', default=None)
@click.option('--html', is_flag=True)
@click.option('--attachment', 'attachments', multiple=True)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def mail_forward(ctx, msg_id, to, body_text, body_file, html, attachments, as_json):
    """Forward an email."""
    try:
        draft_result = rest_request(f'messages/{msg_id}/createForward', method='POST')
        if draft_result['status_code'] != 201:
            return ctx.obj['out']({'ok': False, 'status_code': draft_result['status_code'], 'error': draft_result['body'], 'step': 'createForward'})
        draft_id = (draft_result['body'] or {}).get('Id')
        if not draft_id:
            return ctx.obj['die']('Failed to create forward draft: no draft ID returned')
        patch = {'ToRecipients': parse_recipients(to)}
        content = body_file and open(body_file).read() or body_text
        if content:
            patch['Body'] = {'ContentType': 'HTML' if html else 'Text', 'Content': content}
        patch_result = rest_request(f'messages/{draft_id}', method='PATCH', body=patch)
        if patch_result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': patch_result['status_code'], 'error': patch_result['body'], 'step': 'updateDraft', 'draftId': draft_id})
        attached = add_attachments(draft_id, attachments) if attachments else []
        send_result = rest_request(f'messages/{draft_id}/send', method='POST')
        if send_result['status_code'] != 202:
            return ctx.obj['out']({'ok': False, 'status_code': send_result['status_code'], 'error': send_result['body'], 'step': 'send', 'draftId': draft_id})
        if as_json:
            return ctx.obj['out']({'ok': True, 'sent': True, 'attachments': attached or None})
        recipients = ', '.join(r['EmailAddress']['Address'] for r in parse_recipients(to))
        if attached:
            ctx.obj['text'](f'Forwarded to {recipients} with {len(attached)} attachment(s): {", ".join(a["name"] for a in attached)}')
        else:
            ctx.obj['text'](f'Forwarded to {recipients}')
    except Exception as e:
        ctx.obj['die'](str(e))
