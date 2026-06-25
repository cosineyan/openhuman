import os
import sys
import datetime as _datetime
import click
from ..tokens import ensure_token, set_token, clear_tokens, token_status, extract_tokens_from_chrome

_DEBUG_LOG = os.path.join(os.path.expanduser('~'), '.m365-cli', 'debug.log')


def _marker(msg):
    if os.environ.get('M365_DEBUG') == '0':
        return
    try:
        os.makedirs(os.path.dirname(_DEBUG_LOG), exist_ok=True)
        with open(_DEBUG_LOG, 'a') as f:
            ts = _datetime.datetime.now().isoformat()
            f.write(f'\n===== {ts} pid={os.getpid()} {msg} =====\n')
    except Exception:
        pass


def fmt_status(s):
    if not s.get('cached'):
        return 'not cached'
    if not s.get('valid'):
        return 'expired'
    mins = s.get('expiresInMin')
    return f'valid ({mins}min)' if mins is not None else 'valid'


@click.group()
def auth():
    """Manage M365 authentication tokens."""


@auth.command('status')
@click.option('--json', 'as_json', is_flag=True, help='Output raw JSON')
@click.pass_context
def auth_status(ctx, as_json):
    """Show cached token status."""
    try:
        status = token_status()
        if as_json:
            ctx.obj['out']({'ok': True, **status})
        else:
            ctx.obj['text'](f"Graph: {fmt_status(status['graph'])}")
            ctx.obj['text'](f"REST:  {fmt_status(status['rest'])}")
            ctx.obj['text'](f"Teams: {fmt_status(status['teams'])}")
    except Exception as e:
        ctx.obj['die'](str(e))


@auth.command('login')
@click.option('--json', 'as_json', is_flag=True, help='Output raw JSON')
@click.pass_context
def auth_login(ctx, as_json):
    """Extract tokens from Outlook Web (opens tab if needed)."""
    try:
        extract_tokens_from_chrome()
        status = token_status()
        graph_ok = status.get('graph', {}).get('valid', False)
        rest_ok = status.get('rest', {}).get('valid', False)
        if not graph_ok and not rest_ok:
            msg = ('Could not obtain valid tokens. '
                   'Please ensure you are logged into Outlook Web (outlook.office.com) in Chrome, '
                   'then try again.')
            if as_json:
                ctx.obj['out']({'ok': False, 'error': msg, **status})
            else:
                ctx.obj['die'](msg)
            return
        if as_json:
            ctx.obj['out']({'ok': True, **status})
        else:
            ctx.obj['text']('Login successful.')
            ctx.obj['text'](f"Graph: {fmt_status(status['graph'])}")
            ctx.obj['text'](f"REST:  {fmt_status(status['rest'])}")
    except Exception as e:
        ctx.obj['die'](str(e))


@auth.command('refresh')
@click.option('--json', 'as_json', is_flag=True, help='Output raw JSON')
@click.pass_context
def auth_refresh(ctx, as_json):
    """Force re-extract tokens (even if not expired)."""
    import time as _time
    try:
        _marker('auth refresh START')
        t0 = _time.time()
        ensure_token('graph', force=True)
        t1 = _time.time()
        _marker(f'auth refresh graph done ({int((t1-t0)*1000)}ms)')
        ensure_token('rest', force=True)
        t2 = _time.time()
        _marker(f'auth refresh rest done ({int((t2-t1)*1000)}ms) total={int((t2-t0)*1000)}ms')
        try:
            ensure_token('teams', force=True)
        except Exception as e:
            _marker(f'auth refresh teams WARN: {e}')
        t3 = _time.time()
        _marker(f'auth refresh teams done ({int((t3-t2)*1000)}ms) total={int((t3-t0)*1000)}ms')
        status = token_status()
        if as_json:
            ctx.obj['out']({'ok': True, **status})
        else:
            ctx.obj['text']('Tokens refreshed.')
            ctx.obj['text'](f"Graph: {fmt_status(status['graph'])}")
            ctx.obj['text'](f"REST:  {fmt_status(status['rest'])}")
            ctx.obj['text'](f"Teams: {fmt_status(status['teams'])}")
    except Exception as e:
        _marker(f'auth refresh FAIL: {e}')
        ctx.obj['die'](str(e))


@auth.command('logout')
@click.pass_context
def auth_logout(ctx):
    """Clear cached tokens."""
    clear_tokens()
    ctx.obj['text']('Tokens cleared.')


@auth.command('token')
@click.argument('token_type', default='graph', metavar='[type]')
@click.pass_context
def auth_token(ctx, token_type):
    """Output an access token to stdout (auto-refreshes if expired). Type: graph, rest, teams."""
    valid = ['graph', 'rest', 'teams']
    if token_type not in valid:
        ctx.obj['die'](f'Invalid type "{token_type}". Use: {", ".join(valid)}')
        return
    try:
        token = ensure_token(token_type)
        sys.stdout.write(token)
    except Exception as e:
        ctx.obj['die'](str(e))


@auth.command('set-graph-token')
@click.argument('token')
@click.pass_context
def auth_set_graph(ctx, token):
    """Manually set Graph API token."""
    set_token('graph', token)
    ctx.obj['text']('Graph token saved.')


@auth.command('set-rest-token')
@click.argument('token')
@click.pass_context
def auth_set_rest(ctx, token):
    """Manually set Outlook REST API token."""
    set_token('rest', token)
    ctx.obj['text']('REST token saved.')
