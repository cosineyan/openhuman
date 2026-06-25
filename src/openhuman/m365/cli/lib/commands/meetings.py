from datetime import datetime
import click
from ..meetings import get_meeting_artifacts, get_transcript, get_summary


def fmt_time(iso):
    if not iso:
        return ''
    try:
        d = datetime.fromisoformat(iso.replace('Z', '+00:00'))
        return d.strftime('%b %-d, %H:%M')
    except Exception:
        return iso[:16]


def fmt_offset(offset):
    if not offset:
        return '00:00'
    s = str(offset)
    dot = s.find('.')
    time_part = s[:dot] if dot >= 0 else s
    if time_part.startswith('00:'):
        return time_part[3:]
    return time_part


def duration_min(start_iso, end_iso):
    if not start_iso or not end_iso:
        return None
    try:
        start = datetime.fromisoformat(start_iso.replace('Z', '+00:00'))
        end = datetime.fromisoformat(end_iso.replace('Z', '+00:00'))
        return round((end - start).total_seconds() / 60)
    except Exception:
        return None


@click.group('meetings')
def meetings_cmd():
    """Teams meeting tools."""


@meetings_cmd.command('recap')
@click.argument('chat_id')
@click.option('--list', 'list_mode', is_flag=True, help='List all meeting instances')
@click.option('--summary', 'summary_mode', is_flag=True, help='Show AI-generated meeting summary')
@click.option('--instance', 'instance_num', default=None, help='Instance index (1-based)')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def meetings_recap(ctx, chat_id, list_mode, summary_mode, instance_num, as_json):
    """Show meeting transcript, AI summary, or list meeting instances."""
    try:
        if list_mode:
            data = get_meeting_artifacts(chat_id)
            instances = data['instances']
            if as_json:
                serializable = [{**i, 'startTime': i['startTime'].isoformat() if i['startTime'] else None, 'endTime': i['endTime'].isoformat() if i['endTime'] else None} for i in instances]
                return ctx.obj['out']({'ok': True, 'data': serializable})
            if not instances:
                ctx.obj['text']('No meeting instances found.')
                return
            for i, inst in enumerate(instances):
                start = fmt_time(inst['startTime'].isoformat() if inst['startTime'] else None)
                end = fmt_time(inst['endTime'].isoformat() if inst['endTime'] else None)
                dur = duration_min(inst['startTime'].isoformat() if inst['startTime'] else None, inst['endTime'].isoformat() if inst['endTime'] else None)
                dur_str = f'{dur}min' if dur else ''
                arts = ', '.join(inst['artifacts'].keys())
                ctx.obj['text'](f"{i + 1}. {start} - {end} ({dur_str}, {arts})")
            return

        call_id = None
        if instance_num:
            data = get_meeting_artifacts(chat_id)
            instances = data['instances']
            idx = int(instance_num) - 1
            if idx < 0 or idx >= len(instances):
                raise RuntimeError(f"Instance {instance_num} out of range (1-{len(instances)})")
            call_id = instances[idx]['callId']

        if summary_mode:
            result = get_summary(chat_id, call_id)
            if as_json:
                return ctx.obj['out']({'ok': True, 'data': result})
            header = f"{result['title']} — " if result.get('title') else ''
            header += f"{fmt_time(result['startTime'])} - {fmt_time(result['endTime'])}"
            ctx.obj['text'](header)
            ctx.obj['text']('')
            if result['topics']:
                ctx.obj['text']('## Topics')
                for t in result['topics']:
                    ctx.obj['text'](f"\n### {t['headline']}")
                    ctx.obj['text'](t['summary'])
                    if t['details']:
                        ctx.obj['text']('')
                        for d in t['details']:
                            ctx.obj['text'](f"- **{d['topic']}**: {d['text']}")
                ctx.obj['text']('')
            if result['actionItems']:
                ctx.obj['text']('## Action Items')
                for a in result['actionItems']:
                    ctx.obj['text'](f"- {a['text']}")
            if not result['topics'] and not result['actionItems']:
                ctx.obj['text']('No summary content available.')
            return

        result = get_transcript(chat_id, call_id)
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result})
        if not result['entries']:
            ctx.obj['text']('Transcript is empty.')
            return
        ctx.obj['text'](f"{fmt_time(result['startTime'])} - {fmt_time(result['endTime'])}")
        ctx.obj['text']('')
        for e in result['entries']:
            ts = fmt_offset(e['startOffset'])
            ctx.obj['text'](f"[{ts}] {e['speaker']}: {e['text']}")
    except Exception as e:
        ctx.obj['die'](str(e))
