import urllib.parse
from datetime import datetime, timezone
import click
from ..api import rest_request

VIEW_SELECT = 'Id,Subject,Start,End,Location,Organizer,IsAllDay,IsCancelled,ResponseStatus,ShowAs,Type,IsOnlineMeeting'
DETAIL_SELECT = 'Subject,Start,End,Location,Organizer,Attendees,Body,IsAllDay,IsCancelled,ResponseStatus,ShowAs,Type,IsOnlineMeeting,OnlineMeeting,Recurrence,Importance,Categories,HasAttachments,WebLink,SeriesMasterId'


def strip_html(html):
    import re
    if not html:
        return ''
    text = re.sub(r'<br\s*/?>', '\n', html, re.IGNORECASE)
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


def fmt_date(iso):
    if not iso:
        return ''
    try:
        d = datetime.fromisoformat(iso.replace('Z', '+00:00'))
        return d.strftime('%b %-d, %Y')
    except Exception:
        return iso[:10]


def fmt_time_only(iso):
    if not iso:
        return ''
    try:
        d = datetime.fromisoformat(iso.replace('Z', '+00:00'))
        return d.strftime('%H:%M')
    except Exception:
        return iso[11:16]


def cal_prefix(cal_id):
    return f'calendars/{urllib.parse.quote(cal_id)}/' if cal_id else ''


def parse_date(s, end_of_day=False):
    if not s:
        return None
    if 'T' in s:
        return datetime.fromisoformat(s.replace('Z', '+00:00')).astimezone(timezone.utc).isoformat().replace('+00:00', 'Z')
    suffix = 'T23:59:59' if end_of_day else 'T00:00:00'
    return datetime.fromisoformat(s + suffix).astimezone(timezone.utc).isoformat().replace('+00:00', 'Z')


def response_tag(rs):
    if not rs:
        return ''
    r = rs.get('Response') or ''
    return {'Organizer': '[Organizer]', 'Accepted': '[Accepted]', 'TentativelyAccepted': '[Tentative]',
            'Declined': '[Declined]', 'NotResponded': '[None]'}.get(r, '')


def format_event(e, idx, text_fn):
    cancelled = '[CANCELLED] ' if e.get('IsCancelled') else ''
    subj = e.get('Subject') or '(no subject)'
    resp = response_tag(e.get('ResponseStatus'))
    loc = (e.get('Location') or {}).get('DisplayName') or ''
    online = ' [Teams]' if e.get('IsOnlineMeeting') else ''
    if e.get('IsAllDay'):
        time_str = f'{fmt_date((e.get("Start") or {}).get("DateTime", "") + "Z")} (all day)'
    else:
        start = fmt_time(((e.get('Start') or {}).get('DateTime') or '') + 'Z')
        end = fmt_time_only(((e.get('End') or {}).get('DateTime') or '') + 'Z')
        time_str = f'{start}-{end}'
    text_fn(f'{str(idx).rjust(3)}.  {time_str}  {cancelled}{subj}')
    meta = '  '.join(x for x in [resp, loc, online] if x)
    if meta:
        text_fn(f'      {meta}')
    text_fn(f'      id: {e.get("Id")}')


def format_event_detail(e, text_fn):
    text_fn(e.get('Subject') or '(no subject)')
    if e.get('IsCancelled'):
        text_fn('  [CANCELLED]')
    if e.get('IsAllDay'):
        text_fn(f'  When: {fmt_date(((e.get("Start") or {}).get("DateTime") or "") + "Z")} (all day)')
    else:
        text_fn(f'  Start: {fmt_time(((e.get("Start") or {}).get("DateTime") or "") + "Z")}')
        text_fn(f'  End:   {fmt_time(((e.get("End") or {}).get("DateTime") or "") + "Z")}')
    loc = (e.get('Location') or {}).get('DisplayName')
    if loc:
        text_fn(f'  Location: {loc}')
    org = (e.get('Organizer') or {}).get('EmailAddress') or {}
    if org:
        name = f'{org.get("Name")} <{org.get("Address")}>' if org.get('Name') and org.get('Name') != org.get('Address') else (org.get('Address') or '')
        text_fn(f'  Organizer: {name}')
    resp = response_tag(e.get('ResponseStatus'))
    if resp:
        text_fn(f'  Response: {resp}')
    if e.get('ShowAs') and e['ShowAs'] != 'Busy':
        text_fn(f'  Show as: {e["ShowAs"]}')
    if e.get('Importance') and e['Importance'] != 'Normal':
        text_fn(f'  Importance: {e["Importance"]}')
    if e.get('Categories'):
        text_fn(f'  Categories: {", ".join(e["Categories"])}')
    if e.get('IsOnlineMeeting') and (e.get('OnlineMeeting') or {}).get('JoinUrl'):
        text_fn(f'  Join: {e["OnlineMeeting"]["JoinUrl"]}')
    if e.get('Recurrence'):
        pat = e['Recurrence'].get('Pattern') or {}
        if pat:
            text_fn(f'  Recurrence: {pat.get("Type")} (interval: {pat.get("Interval")})')
    attendees = e.get('Attendees') or []
    if attendees:
        text_fn(f'  Attendees ({len(attendees)}):')
        for a in attendees:
            ea = a.get('EmailAddress') or {}
            name = f'{ea.get("Name")} <{ea.get("Address")}>' if ea.get('Name') and ea.get('Name') != ea.get('Address') else (ea.get('Address') or '?')
            opt = ' (optional)' if a.get('Type') == 'Optional' else ''
            status = (a.get('Status') or {}).get('Response') or ''
            tag = f' [{status}]' if status and status != 'None' else ''
            text_fn(f'    {name}{opt}{tag}')
    body = e.get('Body') or {}
    if body.get('Content'):
        text_fn('')
        text_fn(strip_html(body['Content']) if body.get('ContentType') == 'HTML' else body['Content'])
    if e.get('WebLink'):
        text_fn(f'\n  Web: {e["WebLink"]}')
    text_fn(f'  ID: {e.get("Id")}')
    if e.get('SeriesMasterId'):
        text_fn(f'  SeriesMasterId: {e["SeriesMasterId"]}')


@click.group('calendar')
def calendar_cmd():
    """Calendar events (Outlook REST API v2.0)."""


@calendar_cmd.command('list')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_list(ctx, as_json):
    """List calendars."""
    try:
        result = rest_request('calendars?$select=Name,Id,Color,CanEdit,Owner')
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        calendars = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': calendars})
        if not calendars:
            return ctx.obj['text']('No calendars found.')
        ctx.obj['text']('--- Calendars ---\n')
        for c in calendars:
            name = (c.get('Name') or '(unnamed)').ljust(30)
            edit = 'editable' if c.get('CanEdit') else 'read-only'
            owner = (c.get('Owner') or {}).get('Name') or (c.get('Owner') or {}).get('Address') or ''
            ctx.obj['text'](f'  {name} {edit}  {owner}')
            ctx.obj['text'](f'    id: {c.get("Id")}')
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('today')
@click.option('-n', '--top', default='50')
@click.option('--calendar', 'cal_id', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_today(ctx, top, cal_id, as_json):
    """Today's events."""
    try:
        now = datetime.now(timezone.utc)
        start = datetime(now.year, now.month, now.day, tzinfo=timezone.utc)
        end = datetime(now.year, now.month, now.day, 23, 59, 59, tzinfo=timezone.utc)
        prefix = cal_prefix(cal_id)
        endpoint = (f'{prefix}calendarview?startDateTime={start.isoformat()}&endDateTime={end.isoformat()}'
                    f'&$top={int(top)}&$select={VIEW_SELECT}&$orderby={urllib.parse.quote("Start/DateTime asc")}')
        result = rest_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        events = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': events, 'count': len(events)})
        date_str = now.strftime('%A, %b %-d, %Y')
        if not events:
            return ctx.obj['text'](f'--- {date_str} (no events) ---')
        ctx.obj['text'](f'--- {date_str} ({len(events)} events) ---\n')
        for i, e in enumerate(events):
            format_event(e, i + 1, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('view')
@click.option('--start', 'start_dt', default=None)
@click.option('--end', 'end_dt', default=None)
@click.option('-n', '--top', default='50')
@click.option('--skip', default='0')
@click.option('--calendar', 'cal_id', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_view(ctx, start_dt, end_dt, top, skip, cal_id, as_json):
    """View events in a date range."""
    try:
        if start_dt and end_dt:
            start_iso = parse_date(start_dt, False)
            end_iso = parse_date(end_dt, True)
        elif start_dt:
            start_iso = parse_date(start_dt, False)
            d = datetime.fromisoformat(start_iso.replace('Z', '+00:00'))
            from datetime import timedelta
            end_iso = (d + timedelta(days=7)).isoformat().replace('+00:00', 'Z')
        elif end_dt:
            end_iso = parse_date(end_dt, True)
            d = datetime.fromisoformat(end_iso.replace('Z', '+00:00'))
            from datetime import timedelta
            start_iso = (d - timedelta(days=7)).isoformat().replace('+00:00', 'Z')
        else:
            now = datetime.now(timezone.utc)
            start_iso = datetime(now.year, now.month, now.day, tzinfo=timezone.utc).isoformat().replace('+00:00', 'Z')
            end_iso = datetime(now.year, now.month, now.day, 23, 59, 59, tzinfo=timezone.utc).isoformat().replace('+00:00', 'Z')

        prefix = cal_prefix(cal_id)
        endpoint = (f'{prefix}calendarview?startDateTime={start_iso}&endDateTime={end_iso}'
                    f'&$top={int(top)}&$select={VIEW_SELECT}&$orderby={urllib.parse.quote("Start/DateTime asc")}')
        if int(skip) > 0:
            endpoint += f'&$skip={int(skip)}'
        result = rest_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        events = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': events, 'count': len(events), 'nextSkip': int(skip) + int(top) if next_link else None})
        range_label = f'{start_iso[:10]} — {end_iso[:10]}'
        if not events:
            return ctx.obj['text'](f'--- {range_label} (no events) ---')
        ctx.obj['text'](f'--- {range_label} ({len(events)} events) ---\n')
        for i, e in enumerate(events):
            format_event(e, int(skip) + i + 1, ctx.obj['text'])
            ctx.obj['text']('')
        if next_link:
            ctx.obj['text'](f'More results available. Use: --skip {int(skip) + int(top)}')
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('get')
@click.argument('event_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_get(ctx, event_id, as_json):
    """Get event details."""
    try:
        result = rest_request(f'events/{urllib.parse.quote(event_id)}?$select={DETAIL_SELECT}')
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        format_event_detail(result['body'], ctx.obj['text'])
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('search')
@click.argument('query')
@click.option('-n', '--top', default='20')
@click.option('--skip', default='0')
@click.option('--calendar', 'cal_id', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_search(ctx, query, top, skip, cal_id, as_json):
    """Search events by subject."""
    try:
        prefix = cal_prefix(cal_id)
        escaped = query.replace("'", "''")
        filter_str = f"contains(Subject,'{escaped}')"
        endpoint = (f'{prefix}events?$filter={urllib.parse.quote(filter_str)}'
                    f'&$top={int(top)}&$select={VIEW_SELECT}&$orderby={urllib.parse.quote("Start/DateTime desc")}')
        if int(skip) > 0:
            endpoint += f'&$skip={int(skip)}'
        result = rest_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        events = (result['body'] or {}).get('value') or []
        next_link = (result['body'] or {}).get('@odata.nextLink') or ''
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': events, 'count': len(events), 'nextSkip': int(skip) + int(top) if next_link else None})
        if not events:
            return ctx.obj['text'](f'No events matching "{query}".')
        ctx.obj['text'](f'--- Search: "{query}" ({len(events)} results) ---\n')
        for i, e in enumerate(events):
            format_event(e, int(skip) + i + 1, ctx.obj['text'])
            ctx.obj['text']('')
        if next_link:
            ctx.obj['text'](f'More results available. Use: --skip {int(skip) + int(top)}')
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('accept')
@click.argument('event_id')
@click.option('--comment', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_accept(ctx, event_id, comment, as_json):
    """Accept a meeting invitation."""
    try:
        body = {'SendResponse': True}
        if comment:
            body['Comment'] = comment
        result = rest_request(f'events/{urllib.parse.quote(event_id)}/accept', method='POST', body=body)
        if result['status_code'] != 202:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'accepted': True})
        ctx.obj['text']('Meeting accepted.')
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('decline')
@click.argument('event_id')
@click.option('--comment', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_decline(ctx, event_id, comment, as_json):
    """Decline a meeting invitation."""
    try:
        body = {'SendResponse': True}
        if comment:
            body['Comment'] = comment
        result = rest_request(f'events/{urllib.parse.quote(event_id)}/decline', method='POST', body=body)
        if result['status_code'] != 202:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'declined': True})
        ctx.obj['text']('Meeting declined.')
    except Exception as e:
        ctx.obj['die'](str(e))


@calendar_cmd.command('tentative')
@click.argument('event_id')
@click.option('--comment', default=None)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def calendar_tentative(ctx, event_id, comment, as_json):
    """Tentatively accept a meeting invitation."""
    try:
        body = {'SendResponse': True}
        if comment:
            body['Comment'] = comment
        result = rest_request(f'events/{urllib.parse.quote(event_id)}/tentativelyAccept', method='POST', body=body)
        if result['status_code'] != 202:
            return ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
        if as_json:
            return ctx.obj['out']({'ok': True, 'tentative': True})
        ctx.obj['text']('Meeting tentatively accepted.')
    except Exception as e:
        ctx.obj['die'](str(e))
