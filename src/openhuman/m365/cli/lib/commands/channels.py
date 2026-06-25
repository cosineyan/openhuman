import re
import click
from ..api import graph_request, csa_request


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
        from datetime import datetime
        d = datetime.fromisoformat(iso.replace('Z', '+00:00'))
        return d.strftime('%b %-d, %H:%M')
    except Exception:
        return iso[:16]


@click.group('channels')
def channels_cmd():
    """Teams channels (list, read messages via CSA API)."""


@channels_cmd.command('teams')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def channels_teams(ctx, as_json):
    """List teams you have joined."""
    try:
        result = graph_request('me/joinedTeams?$select=displayName,id,description', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        teams = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': teams})
        if not teams:
            return ctx.obj['text']('No teams found.')
        ctx.obj['text'](f'--- Teams ({len(teams)}) ---\n')
        for t in teams:
            ctx.obj['text'](f'  {t.get("displayName")}')
            ctx.obj['text'](f'    {t.get("id")}')
            if t.get('description'):
                ctx.obj['text'](f'    {t["description"]}')
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@channels_cmd.command('list')
@click.argument('team_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def channels_list(ctx, team_id, as_json):
    """List channels in a team."""
    try:
        result = graph_request(f'groups/{team_id}/team/channels?$select=displayName,id,description,membershipType', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        chs = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': chs})
        if not chs:
            return ctx.obj['text']('No channels found.')
        ctx.obj['text'](f'--- Channels ({len(chs)}) ---\n')
        for c in chs:
            membership = f' ({c["membershipType"]})' if c.get('membershipType') else ''
            ctx.obj['text'](f'  {c.get("displayName")}{membership}')
            ctx.obj['text'](f'    {c.get("id")}')
            if c.get('description'):
                ctx.obj['text'](f'    {c["description"]}')
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@channels_cmd.command('messages')
@click.argument('channel_id')
@click.option('--team', 'team_id', required=True, help='Team Group ID')
@click.option('-n', '--page-size', default='40')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def channels_messages(ctx, channel_id, team_id, page_size, as_json):
    """Read channel messages (CSA API)."""
    try:
        ch_result = graph_request(f'groups/{team_id}/team/channels?$select=displayName,id', raw=True)
        if ch_result['status_code'] != 200:
            return ctx.obj['die'](f'Failed to resolve channels: {ch_result["status_code"]}')
        channels = (ch_result['body'] or {}).get('value') or []
        general = next((c for c in channels if c.get('displayName') == 'General'), None)
        csa_team_id = general['id'] if general else channel_id

        ps = min(int(page_size), 40)
        result = csa_request('emea', f'containers/{channel_id}/posts?modality=conversational&pageSize={ps}&teamId={csa_team_id}')
        if result['status_code'] != 200:
            return ctx.obj['die'](f'CSA API error: {result["status_code"]} {result["body"]}')

        posts = result['body'].get('posts') or []
        filtered = [p for p in posts if (p.get('message') or {}).get('imDisplayName')]

        if as_json:
            return ctx.obj['out']({'ok': True, 'data': filtered})
        if not filtered:
            return ctx.obj['text']('No messages found.')

        ctx.obj['text'](f'--- Channel messages ({len(filtered)}) ---\n')
        for p in filtered:
            msg = p.get('message') or {}
            time = fmt_time(msg.get('createdTime'))
            author = msg.get('imDisplayName') or 'unknown'
            content = strip_html(msg.get('content') or '')
            subject = (msg.get('properties') or {}).get('subject')
            ctx.obj['text'](f'{time}  {author}: {content}')
            if subject:
                ctx.obj['text'](f'  [subject: {subject}]')
    except Exception as e:
        ctx.obj['die'](str(e))
