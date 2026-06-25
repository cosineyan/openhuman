import urllib.parse
import click
from ..api import rest_request

PRESET_COLORS = {
    'preset0': 'Red', 'preset1': 'Orange', 'preset2': 'Brown',
    'preset3': 'Yellow', 'preset4': 'Green', 'preset5': 'Teal',
    'preset6': 'Olive', 'preset7': 'Blue', 'preset8': 'Purple',
    'preset9': 'Cranberry', 'preset10': 'Steel', 'preset11': 'DarkSteel',
    'preset12': 'Gray', 'preset13': 'DarkRed', 'preset14': 'DarkOrange',
    'preset15': 'DarkBrown', 'preset16': 'DarkYellow', 'preset17': 'DarkGreen',
    'preset18': 'DarkTeal', 'preset19': 'DarkOlive', 'preset20': 'DarkBlue',
    'preset21': 'DarkPurple', 'preset22': 'DarkCranberry', 'preset23': 'DarkGray',
    'preset24': 'Black',
}


def resolve_color(input_color):
    if not input_color or input_color == 'none':
        return 'None'
    lower = input_color.lower()
    if lower.startswith('preset'):
        return input_color[0].upper() + input_color[1:].lower()
    for key, label in PRESET_COLORS.items():
        if label.lower() == lower:
            return key[0].upper() + key[1:]
    return input_color


def ensure_category(name, color='none'):
    list_result = rest_request('MasterCategories')
    if list_result['status_code'] != 200:
        return
    cats = (list_result['body'] or {}).get('value') or []
    if not any(c.get('DisplayName') == name for c in cats):
        rest_request('MasterCategories', method='POST',
                     body={'DisplayName': name, 'Color': resolve_color(color)})


@click.group('tag')
def tag_cmd():
    """Manage email categories/tags (Outlook REST API)."""


@tag_cmd.command('list')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def tag_list(ctx, as_json):
    """List all master categories."""
    try:
        result = rest_request('MasterCategories')
        if result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {result['status_code']} {(result['body'] or {}).get('error', {}).get('message') or result['body']}")
        cats = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': cats})
        if not cats:
            return ctx.obj['text']('No categories defined.')
        ctx.obj['text'](f'--- Categories ({len(cats)}) ---\n')
        for c in cats:
            color = PRESET_COLORS.get((c.get('Color') or '').lower()) or c.get('Color') or 'none'
            ctx.obj['text'](f"  {c['DisplayName']}  ({color})")
    except Exception as e:
        ctx.obj['die'](str(e))


@tag_cmd.command('create')
@click.argument('name')
@click.option('-c', '--color', default='none', help='Color (preset0-preset24, or name like Red/Blue/Green)')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def tag_create(ctx, name, color, as_json):
    """Create a new category."""
    try:
        resolved = resolve_color(color)
        result = rest_request('MasterCategories', method='POST',
                              body={'DisplayName': name, 'Color': resolved})
        if result['status_code'] != 201:
            return ctx.obj['die'](f"API error: {result['status_code']} {(result['body'] or {}).get('error', {}).get('message') or result['body']}")
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        body = result['body'] or {}
        color_label = PRESET_COLORS.get((body.get('Color') or '').lower()) or body.get('Color')
        ctx.obj['text'](f"Created: {body.get('DisplayName')}  ({color_label})")
    except Exception as e:
        ctx.obj['die'](str(e))


@tag_cmd.command('delete')
@click.argument('name')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def tag_delete(ctx, name, as_json):
    """Delete a category by name."""
    try:
        list_result = rest_request('MasterCategories')
        if list_result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {list_result['status_code']}")
        cats = (list_result['body'] or {}).get('value') or []
        cat = next((c for c in cats if (c.get('DisplayName') or '').lower() == name.lower()), None)
        if not cat:
            return ctx.obj['die'](f'Category not found: "{name}"')
        result = rest_request(f"MasterCategories('{urllib.parse.quote(cat['Id'])}')", method='DELETE')
        if result['status_code'] != 204:
            return ctx.obj['die'](f"API error: {result['status_code']} {(result['body'] or {}).get('error', {}).get('message') or result['body']}")
        if as_json:
            return ctx.obj['out']({'ok': True, 'deleted': cat['DisplayName']})
        ctx.obj['text'](f"Deleted: {cat['DisplayName']}")
    except Exception as e:
        ctx.obj['die'](str(e))


@tag_cmd.command('get')
@click.argument('message_id')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def tag_get(ctx, message_id, as_json):
    """Show tags on a message."""
    try:
        result = rest_request(f'messages/{message_id}?$select=Subject,Categories')
        if result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {result['status_code']} {(result['body'] or {}).get('error', {}).get('message') or result['body']}")
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': result['body']})
        body = result['body'] or {}
        cats = body.get('Categories') or []
        ctx.obj['text'](f"Subject: {body.get('Subject')}")
        ctx.obj['text'](f"Tags: {', '.join(cats) if cats else '(none)'}")
    except Exception as e:
        ctx.obj['die'](str(e))


@tag_cmd.command('add')
@click.argument('message_id')
@click.argument('name')
@click.option('-c', '--color', default='none', help='Color if category needs to be created')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def tag_add(ctx, message_id, name, color, as_json):
    """Add a tag to a message (auto-creates category if not exists)."""
    try:
        get_result = rest_request(f'messages/{message_id}?$select=Categories')
        if get_result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {get_result['status_code']} {(get_result['body'] or {}).get('error', {}).get('message') or get_result['body']}")
        current = (get_result['body'] or {}).get('Categories') or []
        if name in current:
            return ctx.obj['out']({'ok': True, 'data': current}) if as_json else ctx.obj['text'](f'Already tagged: {name}')

        ensure_category(name, color)
        updated = current + [name]

        result = rest_request(f'messages/{message_id}', method='PATCH', body={'Categories': updated})
        if result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {result['status_code']} {(result['body'] or {}).get('error', {}).get('message') or result['body']}")
        final = (result['body'] or {}).get('Categories') or updated
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': final})
        ctx.obj['text'](f'Added "{name}" → [{", ".join(final)}]')
    except Exception as e:
        ctx.obj['die'](str(e))


@tag_cmd.command('remove')
@click.argument('message_id')
@click.argument('name')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def tag_remove(ctx, message_id, name, as_json):
    """Remove a tag from a message."""
    try:
        get_result = rest_request(f'messages/{message_id}?$select=Categories')
        if get_result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {get_result['status_code']} {(get_result['body'] or {}).get('error', {}).get('message') or get_result['body']}")
        current = (get_result['body'] or {}).get('Categories') or []
        if name not in current:
            return ctx.obj['out']({'ok': True, 'data': current}) if as_json else ctx.obj['text'](f'Tag not present: {name}')

        updated = [c for c in current if c != name]
        result = rest_request(f'messages/{message_id}', method='PATCH', body={'Categories': updated})
        if result['status_code'] != 200:
            return ctx.obj['die'](f"API error: {result['status_code']} {(result['body'] or {}).get('error', {}).get('message') or result['body']}")
        final = (result['body'] or {}).get('Categories') or updated
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': final})
        ctx.obj['text'](f'Removed "{name}" → [{", ".join(final)}]')
    except Exception as e:
        ctx.obj['die'](str(e))
