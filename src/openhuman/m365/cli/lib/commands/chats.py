import re
import urllib.parse
from datetime import datetime
import click
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


@click.group('chats')
def chats_cmd():
    """Microsoft Teams chats."""


@chats_cmd.command('list')
@click.option('--top', default='20')
@click.option('-s', '--select', default=None)
@click.option('--filter', 'odata_filter', default=None)
@click.option('--expand', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def chats_list(ctx, top, select, odata_filter, expand, as_json):
    """List recent chats."""
    try:
        params = [f'$top={top}']
        if select:
            params.append(f'$select={select}')
        if odata_filter:
            params.append(f'$filter={odata_filter}')
        if expand:
            params.append(f'$expand={expand}')
        result = graph_request(f"chats?{'&'.join(params)}", raw=True)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body'].get('value'), 'nextLink': result['body'].get('@odata.nextLink')})
        chats = result['body'].get('value') or []
        for c in chats:
            ctx.obj['text'](f"[{c.get('chatType', 'unknown')}] {c.get('topic', '(no topic)')}  ({fmt_time(c.get('lastUpdatedDateTime'))})")
            ctx.obj['text'](f"  {c['id']}")
        if not chats:
            ctx.obj['text']('No chats found.')
    except Exception as e:
        ctx.obj['die'](str(e))


@chats_cmd.command('get')
@click.argument('chat_id')
@click.option('-s', '--select', default=None)
@click.option('--expand', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def chats_get(ctx, chat_id, select, expand, as_json):
    """Get a specific chat by ID."""
    try:
        params = []
        if select:
            params.append(f'$select={select}')
        if expand:
            params.append(f'$expand={expand}')
        qs = '?' + '&'.join(params) if params else ''
        result = graph_request(f'chats/{chat_id}{qs}', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        c = result['body']
        ctx.obj['text'](f"Chat: {c.get('topic', '(no topic)')}")
        ctx.obj['text'](f"Type: {c.get('chatType', 'unknown')}")
        if c.get('createdDateTime'):
            ctx.obj['text'](f"Created: {fmt_time(c['createdDateTime'])}")
        if c.get('lastUpdatedDateTime'):
            ctx.obj['text'](f"Updated: {fmt_time(c['lastUpdatedDateTime'])}")
        if c.get('id'):
            ctx.obj['text'](f"ID: {c['id']}")
    except Exception as e:
        ctx.obj['die'](str(e))


@chats_cmd.command('messages')
@click.argument('chat_id', required=False)
@click.option('--top', default='20')
@click.option('--per-chat', 'per_chat', default='5')
@click.option('--since', default=None)
@click.option('--filter', 'odata_filter', default=None)
@click.option('-s', '--select', default=None)
@click.option('--members', is_flag=True, help='Include member count per chat (all-chats mode)')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def chats_messages(ctx, chat_id, top, per_chat, since, odata_filter, select, members, as_json):
    """List messages in a chat, or fetch recent messages across all chats."""
    try:
        if chat_id:
            params = [f'$top={top}', '$orderby=lastModifiedDateTime desc']
            if select:
                params.append(f'$select={select}')
            filters = []
            if odata_filter:
                filters.append(odata_filter)
            if since:
                filters.append(f'lastModifiedDateTime gt {since}')
            if filters:
                params.append(f"$filter={' and '.join(filters)}")
            result = graph_request(f"chats/{chat_id}/messages?{'&'.join(params)}", raw=True)
            if result['status_code'] != 200:
                return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
            msgs = result['body'].get('value') or []
            if as_json:
                return ctx.obj['out']({'ok': True, 'data': msgs, 'nextLink': result['body'].get('@odata.nextLink')})
            for m in msgs:
                time_str = fmt_time(m.get('createdDateTime'))
                sender = (m.get('from') or {}).get('user', {}).get('displayName') or (m.get('from') or {}).get('application', {}).get('displayName') or 'system'
                body = strip_html((m.get('body') or {}).get('content'))
                mid = f"  [{m['id']}]" if m.get('id') else ''
                if body:
                    ctx.obj['text'](f"{time_str}  {sender}: {body}{mid}")
            if not msgs:
                ctx.obj['text']('No messages found.')
            return

        # All-chats mode — filter by lastMessagePreview at chat level
        chat_params = [f'$top={top}', '$expand=lastMessagePreview', '$orderby=lastMessagePreview/createdDateTime desc']
        chat_filters = []
        if odata_filter:
            chat_filters.append(odata_filter)
        if since:
            chat_filters.append(f'lastMessagePreview/createdDateTime gt {since}')
        if chat_filters:
            chat_params.append(f"$filter={' and '.join(chat_filters)}")
        chat_result = graph_request(f"chats?{'&'.join(chat_params)}", raw=True)
        if chat_result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': chat_result['status_code'], 'error': chat_result['body']})

        chat_list = chat_result['body'].get('value') or []
        if not chat_list:
            if as_json:
                return ctx.obj['out']({'ok': True, 'data': []})
            ctx.obj['text']('No chats found.')
            return

        msg_params = [f'$top={per_chat}']
        if since:
            msg_params.append('$orderby=lastModifiedDateTime desc')
            msg_params.append(f'$filter=lastModifiedDateTime gt {since}')
        else:
            msg_params.append('$orderby=createdDateTime desc')
        if select:
            msg_params.append(f'$select={select}')
        msg_qs = '&'.join(msg_params)

        # Fetch member counts if --members flag is set
        member_counts = {}
        if members:
            import json as _json
            for c in chat_list:
                if c.get('chatType') == 'oneOnOne':
                    member_counts[c['id']] = 2
            need_fetch = [c for c in chat_list if c.get('chatType') != 'oneOnOne']
            if need_fetch:
                batch_body = {
                    'requests': [
                        {'id': str(i), 'method': 'GET', 'url': f"/chats/{c['id']}/members?$select=id"}
                        for i, c in enumerate(need_fetch)
                    ]
                }
                batch_result = graph_request('$batch', method='POST', body=batch_body, raw=True)
                if batch_result['status_code'] == 200 and (batch_result['body'] or {}).get('responses'):
                    for resp in batch_result['body']['responses']:
                        idx = int(resp['id'])
                        member_counts[need_fetch[idx]['id']] = len((resp.get('body') or {}).get('value') or [])

        results = []
        for c in chat_list:
            r = graph_request(f"chats/{c['id']}/messages?{msg_qs}", raw=True)
            results.append({
                'chat': c,
                'messages': r['body'].get('value') or [] if r['status_code'] == 200 else [],
                'error': r['body'] if r['status_code'] != 200 else None,
            })

        if as_json:
            data = [{'chatId': r['chat']['id'], 'chatType': r['chat'].get('chatType'), 'topic': r['chat'].get('topic'),
                     **(({'memberCount': member_counts.get(r['chat']['id'])} ) if members else {}),
                     'messages': r['messages']} for r in results]
            return ctx.obj['out']({'ok': True, 'data': data})

        any_messages = False
        for r in results:
            msgs = [m for m in r['messages'] if strip_html((m.get('body') or {}).get('content'))]
            if not msgs:
                continue
            any_messages = True
            topic = r['chat'].get('topic') or '(no topic)'
            chat_type = r['chat'].get('chatType') or 'unknown'
            mc = f" ({member_counts.get(r['chat']['id'], '?')} members)" if members else ''
            ctx.obj['text'](f"\n--- [{chat_type}] {topic}{mc} ---")
            ctx.obj['text'](f"    {r['chat']['id']}")
            for m in msgs:
                time_str = fmt_time(m.get('createdDateTime'))
                sender = (m.get('from') or {}).get('user', {}).get('displayName') or (m.get('from') or {}).get('application', {}).get('displayName') or 'system'
                body = strip_html((m.get('body') or {}).get('content'))
                mid = f"  [{m['id']}]" if m.get('id') else ''
                ctx.obj['text'](f"  {time_str}  {sender}: {body}{mid}")
        if not any_messages:
            ctx.obj['text']('No messages found.')
    except Exception as e:
        ctx.obj['die'](str(e))


@chats_cmd.command('members')
@click.argument('chat_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def chats_members(ctx, chat_id, as_json):
    """List members of a chat."""
    try:
        result = graph_request(f'chats/{chat_id}/members', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body'].get('value')})
        members = result['body'].get('value') or []
        for m in members:
            name = m.get('displayName') or '(unknown)'
            email = f" ({m['email']})" if m.get('email') else ''
            roles = f" [{','.join(m['roles'])}]" if m.get('roles') else ''
            ctx.obj['text'](f"{name}{email}{roles}")
        if not members:
            ctx.obj['text']('No members found.')
    except Exception as e:
        ctx.obj['die'](str(e))


@chats_cmd.command('send')
@click.argument('chat_id')
@click.argument('message')
@click.option('--content-type', default='text')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def chats_send(ctx, chat_id, message, content_type, as_json):
    """Send a message to a chat."""
    try:
        result = graph_request(f'chats/{chat_id}/messages', method='POST',
                               body={'body': {'contentType': content_type, 'content': message}}, raw=True)
        if result['status_code'] != 201:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        ctx.obj['text'](f"Message sent at {fmt_time(result['body'].get('createdDateTime'))}")
    except Exception as e:
        ctx.obj['die'](str(e))


@chats_cmd.command('reply')
@click.argument('chat_id')
@click.argument('message_id')
@click.argument('message')
@click.option('--content-type', default='text')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def chats_reply(ctx, chat_id, message_id, message, content_type, as_json):
    """Reply (with quote) to a message in a chat."""
    try:
        import json as _json
        orig = graph_request(f'chats/{chat_id}/messages/{message_id}', raw=True)
        if orig['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': orig['status_code'], 'error': orig['body'], 'step': 'fetchOriginal'})

        orig_body = orig['body']
        sender_user = (orig_body.get('from') or {}).get('user') or {}
        preview = strip_html((orig_body.get('body') or {}).get('content') or '')[:200]

        ref_content = _json.dumps({
            'messageId': message_id,
            'messagePreview': preview,
            'messageSender': {
                'application': (orig_body.get('from') or {}).get('application'),
                'device': (orig_body.get('from') or {}).get('device'),
                'user': {
                    'userIdentityType': sender_user.get('userIdentityType') or 'aadUser',
                    'tenantId': sender_user.get('tenantId'),
                    'id': sender_user.get('id'),
                    'displayName': sender_user.get('displayName'),
                },
            },
        })

        reply_body = f'<attachment id="{message_id}"></attachment>{message}'
        result = graph_request(f'chats/{chat_id}/messages', method='POST', body={
            'body': {'contentType': 'html', 'content': reply_body},
            'attachments': [{'id': message_id, 'contentType': 'messageReference', 'contentUrl': None, 'content': ref_content, 'name': None, 'thumbnailUrl': None}],
        }, raw=True)
        if result['status_code'] != 201:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        ctx.obj['text'](f"Reply sent at {fmt_time(result['body'].get('createdDateTime'))}")
    except Exception as e:
        ctx.obj['die'](str(e))
