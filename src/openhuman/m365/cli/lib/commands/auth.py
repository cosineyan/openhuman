import os
import sys
import datetime as _datetime
import click
from ..tokens import ensure_token, ensure_teams_token, ensure_graph_token, set_token, clear_tokens, token_status, extract_tokens_from_chrome

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
        # Get graph token via Teams refresh token (works even when Outlook page doesn't expose it).
        try:
            ensure_graph_token(force=True)
        except Exception:
            pass
        # Also try to get teams token.
        try:
            ensure_teams_token(force=True)
        except Exception:
            pass
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
    try:
        # If rest token is already cached and usable, skip Chrome extraction
        # and just exchange for fresh graph/teams tokens (much faster).
        from ..tokens import load_tokens, is_token_usable
        cached = load_tokens()
        rest_usable = is_token_usable(cached.get('rest'))
        if not rest_usable:
            # Need to open Chrome tab to get rest token.
            extract_tokens_from_chrome()
        # Get graph and teams via token exchange (no Chrome tab needed).
        try:
            ensure_graph_token(force=True)
        except Exception:
            pass
        try:
            ensure_teams_token(force=True)
        except Exception:
            pass
        status = token_status()
        rest_ok = status.get('rest', {}).get('valid', False)
        if not rest_ok:
            msg = ('Could not obtain valid tokens. '
                   'Please ensure you are logged into Outlook Web in Chrome, then try again.')
            if as_json:
                ctx.obj['out']({'ok': False, 'error': msg, **status})
            else:
                ctx.obj['die'](msg)
            return
        if as_json:
            ctx.obj['out']({'ok': True, **status})
        else:
            ctx.obj['text']('Tokens refreshed.')
            ctx.obj['text'](f"Graph: {fmt_status(status['graph'])}")
            ctx.obj['text'](f"REST:  {fmt_status(status['rest'])}")
            ctx.obj['text'](f"Teams: {fmt_status(status['teams'])}")
    except Exception as e:
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
