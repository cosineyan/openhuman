import os
import sys
import datetime as _datetime
import click
from ..tokens import (ensure_token, ensure_teams_token, ensure_graph_token,
                      set_token, clear_tokens, token_status, extract_tokens_from_chrome,
                      get_aha_token, set_aha_token, clear_aha_token,
                      check_jira_accessible, check_wiki_accessible,
                      ensure_spo_token, load_tokens, is_token_usable,
                      REFRESH_THRESHOLD_MIN,
                      get_github_token, set_github_token, clear_github_token,
                      check_github_accessible,
                      find_outlook_session, open_outlook_tab,
                      extract_from_session, close_tab,
                      ensure_chat_graph_token)

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
    """Show cached token status including SAP additional services."""
    try:
        status = token_status()
        # Aha! API token
        aha_tok = get_aha_token()
        status['aha'] = {'valid': bool(aha_tok), 'cached': bool(aha_tok)}
        # Jira SSO (via mcp-chrome fetch to REST API)
        jira_ok = check_jira_accessible()
        status['jira'] = {'valid': jira_ok, 'cached': jira_ok}
        # Confluence Wiki SSO (via mcp-chrome fetch to REST API)
        wiki_ok = check_wiki_accessible()
        status['wiki'] = {'valid': wiki_ok, 'cached': wiki_ok}
        # SharePoint SPO token
        tokens = load_tokens()
        spo_entry = tokens.get('spo:sap.sharepoint.com')
        spo_usable = is_token_usable(spo_entry)
        status['sharepoint'] = {
            'valid': spo_usable,
            'cached': bool(spo_entry),
            'expiresInMin': round((spo_entry['expiresOn'] - __import__('time').time()) / 60)
                if spo_entry and spo_entry.get('expiresOn') and spo_usable else None,
        }
        # SAP GitHub PATs — token 存在即视为 valid，不发 API 请求（避免 rate limit）
        # 用户手动保存的 PAT 不会自动过期，无需每次轮询都验证
        tools_cached = bool(get_github_token('tools'))
        status['githubTools'] = {'valid': tools_cached, 'cached': tools_cached}
        wdf_cached = bool(get_github_token('wdf'))
        status['githubWdf'] = {'valid': wdf_cached, 'cached': wdf_cached}
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
    """Extract tokens from Outlook Web (opens tab if needed, waits until tokens appear)."""
    try:
        import time as _time
        from ..tokens import save_tokens, _reconnect_extension

        def _try_extract(sid):
            """Try exec; if result is None (sleep/wake broke native messaging), reconnect and retry once."""
            data = extract_from_session(sid)
            if data is None:
                _reconnect_extension()
                _time.sleep(2)
                data = extract_from_session(sid)
            return data

        # Fast path: existing tab with valid tokens.
        existing_sid = find_outlook_session()
        if existing_sid:
            data = _try_extract(existing_sid)
            now = int(_time.time())
            rest_entry = (data or {}).get('rest') or {}
            graph_entry = (data or {}).get('graph') or {}
            rest_ok = rest_entry.get('token') and rest_entry.get('expiresOn', 0) > now
            graph_ok = graph_entry.get('token') and graph_entry.get('expiresOn', 0) > now
            if rest_ok and graph_ok:
                tokens = load_tokens()
                tokens['rest'] = {**rest_entry, 'sessionId': existing_sid}
                tokens['graph'] = {**graph_entry, 'sessionId': existing_sid}
                save_tokens(tokens)
                try:
                    ensure_graph_token(force=True)
                except Exception:
                    pass
                try:
                    ensure_teams_token(force=True)
                except Exception:
                    pass
                try:
                    ensure_chat_graph_token(force=True)
                except Exception:
                    pass
                st = token_status()
                if as_json:
                    ctx.obj['out']({'ok': True, **st})
                else:
                    ctx.obj['text']('Login successful.')
                return

        # Slow path: no valid tokens in existing tab — use extract_tokens_from_chrome
        # which will wait on the existing tab or open a new one.
        extract_tokens_from_chrome()
        try:
            ensure_graph_token(force=True)
        except Exception:
            pass
        try:
            ensure_teams_token(force=True)
        except Exception:
            pass
        try:
            ensure_chat_graph_token(force=True)
        except Exception:
            pass
        st = token_status()
        rest_ok = st.get('rest', {}).get('valid', False)
        graph_ok = st.get('graph', {}).get('valid', False)
        if not rest_ok and not graph_ok:
            msg = ('Could not obtain valid tokens. '
                   'Please ensure you are logged into Outlook Web in Chrome, then try again.')
            if as_json:
                ctx.obj['out']({'ok': False, 'error': msg, **st})
            else:
                ctx.obj['die'](msg)
            return
        if as_json:
            ctx.obj['out']({'ok': True, **st})
        else:
            ctx.obj['text']('Login successful.')
    except Exception as e:
        ctx.obj['die'](str(e))


@auth.command('login-watch')
@click.option('--session-id', 'session_id', default=None, help='Specific Chrome session to watch')
@click.option('--timeout', 'timeout_sec', default=120, type=int, help='Max seconds to wait')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_login_watch(ctx, session_id, timeout_sec, as_json):
    """Block until Outlook tokens appear in Chrome (for use after auth login returns waiting=true)."""
    import time as _time
    try:
        deadline = _time.time() + timeout_sec
        sid = session_id or find_outlook_session()
        if not sid:
            # Wait for any Outlook tab to appear.
            while _time.time() < deadline:
                sid = find_outlook_session()
                if sid:
                    break
                _time.sleep(2)
        if not sid:
            ctx.obj['die']('No Outlook tab found within timeout.')
            return

        # Poll for valid tokens on that tab.
        while _time.time() < deadline:
            data = extract_from_session(sid)
            now = int(_time.time())
            rest_entry = (data or {}).get('rest') or {}
            graph_entry = (data or {}).get('graph') or {}
            rest_ok = rest_entry.get('token') and rest_entry.get('expiresOn', 0) > now
            graph_ok = graph_entry.get('token') and graph_entry.get('expiresOn', 0) > now
            if rest_ok and graph_ok:
                tokens = load_tokens()
                tokens['rest'] = {**rest_entry, 'sessionId': sid}
                tokens['graph'] = {**graph_entry, 'sessionId': sid}
                from ..tokens import save_tokens
                save_tokens(tokens)
                try:
                    ensure_graph_token(force=True)
                except Exception:
                    pass
                try:
                    ensure_teams_token(force=True)
                except Exception:
                    pass
                st = token_status()
                if as_json:
                    ctx.obj['out']({'ok': True, 'waiting': False, **st})
                else:
                    ctx.obj['text']('Tokens acquired.')
                return
            _time.sleep(3)

        ctx.obj['die'](f'Timed out after {timeout_sec}s waiting for Outlook tokens.')
    except Exception as e:
        ctx.obj['die'](str(e))


@auth.command('refresh')
@click.option('--json', 'as_json', is_flag=True, help='Output raw JSON')
@click.pass_context
def auth_refresh(ctx, as_json):
    """Re-acquire tokens that are expired or expiring within 5 minutes."""
    import fcntl
    # File lock: prevent concurrent auth refresh processes (UI polls every 5s
    # when a token is expired, which would spawn multiple refresh subprocesses).
    lock_path = os.path.join(CONFIG_DIR, 'refresh.lock')
    try:
        lock_fd = open(lock_path, 'w')
        fcntl.flock(lock_fd, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except OSError:
        # Another refresh is already running — return current status immediately
        st = token_status()
        if as_json:
            ctx.obj['out']({'ok': True, 'skipped': True, **st})
        return
    try:
        cached = load_tokens()

        def _is_cached(key):
            """Token has been stored at some point (may be expired)."""
            return bool(cached.get(key))

        def _needs_refresh(key):
            entry = cached.get(key)
            if not entry:
                return False  # never stored — skip, don't open Chrome
            if not is_token_usable(entry):
                return True  # cached but expired
            exp = (entry or {}).get('expiresOn')
            if exp is None:
                return False
            return (exp - __import__('time').time()) < REFRESH_THRESHOLD_MIN * 60

        errors = []

        # REST token: only try Chrome if we have previously cached a token
        if _needs_refresh('rest'):
            try:
                extract_tokens_from_chrome()
            except Exception as e:
                errors.append(f'rest: {e}')

        # Token-exchange tokens: refresh if expiring soon
        if _needs_refresh('graph'):
            try:
                ensure_graph_token(force=True)
            except Exception as e:
                errors.append(f'graph: {e}')

        if _needs_refresh('teams'):
            try:
                ensure_teams_token(force=True)
            except Exception as e:
                errors.append(f'teams: {e}')

        if _needs_refresh('spo:sap.sharepoint.com'):
            try:
                ensure_spo_token('sap.sharepoint.com')
            except Exception as e:
                errors.append(f'sharepoint: {e}')

        # Refresh graph_chat only if expired or expiring soon — not unconditionally.
        # Opening a foreground Outlook tab every refresh poll is very disruptive.
        if _needs_refresh('graph_chat'):
            try:
                ensure_chat_graph_token(force=True)
            except Exception as e:
                errors.append(f'graph_chat: {e}')

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
            result = {'ok': True, **status}
            if errors:
                result['warnings'] = errors
            ctx.obj['out'](result)
        else:
            ctx.obj['text']('Tokens refreshed.')
            if errors:
                for err in errors:
                    ctx.obj['text'](f'Warning: {err}')
    except Exception as e:
        ctx.obj['die'](str(e))
    finally:
        try:
            fcntl.flock(lock_fd, fcntl.LOCK_UN)
            lock_fd.close()
        except Exception:
            pass


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


@auth.command('set-aha-token')
@click.argument('token')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_set_aha(ctx, token, as_json):
    """Save an Aha! API token."""
    set_aha_token(token)
    if as_json:
        ctx.obj['out']({'ok': True, 'saved': True})
    else:
        ctx.obj['text']('Aha! token saved.')


@auth.command('clear-aha-token')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_clear_aha(ctx, as_json):
    """Remove the stored Aha! API token."""
    clear_aha_token()
    if as_json:
        ctx.obj['out']({'ok': True, 'cleared': True})
    else:
        ctx.obj['text']('Aha! token cleared.')


@auth.command('refresh-sharepoint')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_refresh_spo(ctx, as_json):
    """Re-exchange Teams refresh token for a fresh SharePoint token."""
    try:
        ensure_spo_token('sap.sharepoint.com')
        if as_json:
            ctx.obj['out']({'ok': True})
        else:
            ctx.obj['text']('SharePoint token refreshed.')
    except Exception as e:
        ctx.obj['die'](str(e))


@auth.command('set-github-tools-token')
@click.argument('token')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_set_github_tools(ctx, token, as_json):
    """Save a Personal Access Token for github.tools.sap."""
    set_github_token('tools', token)
    if as_json:
        ctx.obj['out']({'ok': True, 'saved': True})
    else:
        ctx.obj['text']('github.tools.sap token saved.')


@auth.command('clear-github-tools-token')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_clear_github_tools(ctx, as_json):
    """Remove the stored PAT for github.tools.sap."""
    clear_github_token('tools')
    if as_json:
        ctx.obj['out']({'ok': True, 'cleared': True})
    else:
        ctx.obj['text']('github.tools.sap token cleared.')


@auth.command('set-github-wdf-token')
@click.argument('token')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_set_github_wdf(ctx, token, as_json):
    """Save a Personal Access Token for github.wdf.sap.corp."""
    set_github_token('wdf', token)
    if as_json:
        ctx.obj['out']({'ok': True, 'saved': True})
    else:
        ctx.obj['text']('github.wdf.sap.corp token saved.')


@auth.command('clear-github-wdf-token')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def auth_clear_github_wdf(ctx, as_json):
    """Remove the stored PAT for github.wdf.sap.corp."""
    clear_github_token('wdf')
    if as_json:
        ctx.obj['out']({'ok': True, 'cleared': True})
    else:
        ctx.obj['text']('github.wdf.sap.corp token cleared.')
