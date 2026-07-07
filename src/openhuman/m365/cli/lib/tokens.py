import os
import json
import time
import base64
import subprocess
import threading
import datetime as _datetime
import urllib.request
import urllib.parse

CONFIG_DIR = os.path.dirname(
    os.environ.get('M365_TOKEN_FILE')
    or os.path.join(os.path.expanduser('~'), '.m365-cli', 'tokens.json')
)
TOKEN_FILE = os.environ.get('M365_TOKEN_FILE') or os.path.join(
    os.path.expanduser('~'), '.m365-cli', 'tokens.json'
)
# Long-lived credentials (Aha!, GitHub PATs) stored separately so they are
# never touched by clear_tokens() or any M365 session refresh flow.
CREDENTIALS_FILE = os.path.join(CONFIG_DIR, 'credentials.json')
DEBUG_LOG = os.path.join(CONFIG_DIR, 'debug.log')
REFRESH_THRESHOLD_MIN = 5

_T0 = _datetime.datetime.now()
_PID = os.getpid()


def dlog(msg):
    if os.environ.get('M365_DEBUG') == '0':
        return
    try:
        os.makedirs(CONFIG_DIR, exist_ok=True)
        ts = _datetime.datetime.now().isoformat()
        elapsed = int((_datetime.datetime.now() - _T0).total_seconds() * 1000)
        line = f'{ts} pid={_PID} +{elapsed}ms {msg}\n'
        with open(DEBUG_LOG, 'a') as f:
            f.write(line)
    except Exception:
        pass

MCP_CHROME_PORT = os.environ.get('MCP_CHROME_PORT') or os.environ.get('CHROME_MCP_PORT') or '12306'
MCP_BROWSER_URL = f'http://127.0.0.1:{MCP_CHROME_PORT}/browser'

EXTRACT_JS = 'outlook'
EXTRACT_TEAMS_RT_JS = 'teams_rt'


# --- Cache file I/O ---

def load_tokens():
    try:
        with open(TOKEN_FILE, 'r') as f:
            return json.loads(f.read())
    except Exception:
        return {}


def save_tokens(tokens):
    os.makedirs(CONFIG_DIR, exist_ok=True)
    with open(TOKEN_FILE, 'w') as f:
        f.write(json.dumps(tokens, indent=2) + '\n')


def load_credentials():
    """Load long-lived credentials (Aha!, GitHub PATs) from a separate file."""
    try:
        with open(CREDENTIALS_FILE, 'r') as f:
            return json.loads(f.read())
    except Exception:
        return {}


def save_credentials(creds):
    os.makedirs(CONFIG_DIR, exist_ok=True)
    with open(CREDENTIALS_FILE, 'w') as f:
        f.write(json.dumps(creds, indent=2) + '\n')


# --- Token validity ---

def is_token_usable(entry):
    if not entry or not entry.get('token'):
        return False
    expires_on = entry.get('expiresOn')
    if expires_on is None:
        return True
    return expires_on > int(time.time())


def is_token_expiring_soon(entry):
    if not entry or not entry.get('token'):
        return False
    expires_on = entry.get('expiresOn')
    if expires_on is None:
        return False
    remaining = expires_on - int(time.time())
    return 0 < remaining < REFRESH_THRESHOLD_MIN * 60


def expires_in_min(entry):
    if not entry or not entry.get('token'):
        return None
    expires_on = entry.get('expiresOn')
    if expires_on is None:
        return None
    return round((expires_on - int(time.time())) / 60)


# --- Public API ---

def get_token(token_type):
    tokens = load_tokens()
    entry = tokens.get(token_type)
    return entry['token'] if is_token_usable(entry) else None


def token_status():
    tokens = load_tokens()
    status = {}
    for t in ['graph', 'rest', 'teams']:
        entry = tokens.get(t)
        if not entry or not entry.get('token'):
            status[t] = {'valid': False, 'cached': False}
        else:
            status[t] = {
                'valid': is_token_usable(entry),
                'cached': True,
                'expiresInMin': expires_in_min(entry),
                'sessionId': entry.get('sessionId'),
            }
    return status


def set_token(token_type, token_str):
    tokens = load_tokens()
    tokens[token_type] = {'token': token_str, 'expiresOn': None, 'sessionId': None}
    save_tokens(tokens)


def clear_tokens():
    """Clear all M365 session tokens by removing the token file.
    Credentials (Aha!, GitHub PATs) live in a separate file and are unaffected."""
    try:
        os.unlink(TOKEN_FILE)
    except Exception:
        pass


# --- mcp-chrome HTTP bridge ---

def mcp_browser_cmd(body, timeout_ms=30000):
    data = json.dumps(body).encode('utf-8')
    req = urllib.request.Request(
        MCP_BROWSER_URL,
        data=data,
        headers={'Content-Type': 'application/json'},
        method='POST',
    )
    with urllib.request.urlopen(req, timeout=timeout_ms / 1000) as resp:
        return json.loads(resp.read().decode('utf-8'))


def find_outlook_session():
    resp = mcp_browser_cmd({'command': 'sessions'})
    if not resp.get('ok') or not resp.get('sessions'):
        return None
    for s in resp['sessions']:
        url = s.get('url', '')
        # Must be the actual Outlook app, not an OAuth redirect page.
        if url.startswith('https://outlook.office.com/') or url.startswith('https://outlook.cloud.microsoft/'):
            return s['id']
    return None


def open_outlook_tab():
    resp = mcp_browser_cmd({'command': 'new-tab', 'url': 'https://outlook.office.com/mail/'})
    if not resp.get('ok') or not resp.get('data'):
        raise RuntimeError('Failed to open Outlook tab')
    return resp['data']['sessionId']


def find_teams_session():
    resp = mcp_browser_cmd({'command': 'sessions'})
    if not resp.get('ok') or not resp.get('sessions'):
        return None
    for s in resp['sessions']:
        url = s.get('url', '')
        # Must be the actual Teams app, not an OAuth redirect page that happens
        # to contain teams.microsoft.com as a redirect_uri parameter.
        if url.startswith('https://teams.microsoft.com/') or url.startswith('https://teams.cloud.microsoft/'):
            return s['id']
    return None


def open_teams_tab():
    resp = mcp_browser_cmd({'command': 'new-tab', 'url': 'https://teams.microsoft.com/'})
    if not resp.get('ok') or not resp.get('data'):
        raise RuntimeError('Failed to open Teams tab')
    return resp['data']['sessionId']


def extract_teams_rt_from_session(sid):
    resp = mcp_browser_cmd({'command': 'exec', 'sessionId': sid, 'code': EXTRACT_TEAMS_RT_JS, 'timeout': 15}, 30000)
    if not resp.get('ok') or resp.get('data') is None:
        return None
    data = resp['data']
    return json.loads(data) if isinstance(data, str) else data


def close_tab(sid):
    try:
        mcp_browser_cmd({'command': 'close-tab', 'sessionId': sid})
    except Exception:
        pass


def navigate_tab(sid, url):
    dlog(f'navigate_tab sid={sid} url={url}')
    mcp_browser_cmd({'command': 'navigate', 'url': url, 'sessionId': sid}, 15000)


def _reconnect_extension():
    """Re-establish the mcp-chrome extension ↔ native host connection after sleep/wake.

    After macOS sleeps, the native messaging pipe between Chrome extension and the
    Node native host drops. The extension reconnects automatically when it gets
    activity — the simplest trigger is navigating an existing tab, which fires
    Chrome's webNavigation events and wakes the extension's background service worker.
    """
    try:
        resp = mcp_browser_cmd({'command': 'sessions'}, 5000)
        sessions = resp.get('sessions', [])
        if sessions:
            # Navigate an existing tab to the same URL — triggers extension wake-up.
            sid = sessions[0]['id']
            url = sessions[0].get('url', 'about:blank')
            mcp_browser_cmd({'command': 'navigate', 'url': url, 'sessionId': sid}, 8000)
            time.sleep(1)
    except Exception:
        pass


def extract_from_session(sid):
    resp = mcp_browser_cmd({'command': 'exec', 'sessionId': sid, 'code': EXTRACT_JS, 'timeout': 15}, 30000)
    if not resp.get('ok') or resp.get('data') is None:
        return None
    data = resp['data']
    result = json.loads(data) if isinstance(data, str) else data
    # If both tokens are null the extension injected but found nothing —
    # this can happen right after sleep/wake when the page hasn't re-authed.
    # Return the result as-is; callers will retry or open a new tab.
    return result


def open_and_wait_for_token(token_type, max_wait_sec=90):
    """Open a new Outlook tab, wait for a valid token, then close the tab."""
    sid = open_outlook_tab()
    try:
        deadline = time.time() + max_wait_sec
        time.sleep(3)
        while time.time() < deadline:
            data = extract_from_session(sid)
            entry = (data or {}).get(token_type) or {}
            if entry.get('token') and entry.get('expiresOn', 0) > int(time.time()):
                close_tab(sid)
                return sid, data
            time.sleep(2)
        data = extract_from_session(sid)
        close_tab(sid)
        return sid, data
    except Exception:
        close_tab(sid)
        raise


# --- Background refresh ---

def background_refresh():
    script = f"""
import os, json, time, urllib.request
PORT = os.environ.get('MCP_CHROME_PORT') or os.environ.get('CHROME_MCP_PORT') or '12306'
BASE = f'http://127.0.0.1:{{PORT}}/browser'
TOKEN_FILE = os.path.join(os.path.expanduser('~'), '.m365-cli', 'tokens.json')

def cmd(body):
    data = json.dumps(body).encode()
    req = urllib.request.Request(BASE, data=data, headers={{'Content-Type': 'application/json'}}, method='POST')
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())

try:
    nr = cmd({{'command': 'new-tab', 'url': 'https://outlook.office.com/mail/'}})
    if not nr.get('ok'): exit()
    sid = nr['data']['sessionId']
    deadline = time.time() + 30
    time.sleep(3)
    while time.time() < deadline:
        r = cmd({{'command': 'exec', 'sessionId': sid, 'code': 'outlook', 'timeout': 15}})
        if r.get('ok') and r.get('data'):
            d = r['data']
            if isinstance(d, str): d = json.loads(d)
            now = int(time.time())
            if d.get('graph', {{}}).get('expiresOn', 0) > now and d.get('rest', {{}}).get('expiresOn', 0) > now:
                tokens = {{}}
                try:
                    with open(TOKEN_FILE) as f: tokens = json.loads(f.read())
                except: pass
                tokens['graph'] = {{**d['graph'], 'sessionId': sid}}
                tokens['rest'] = {{**d['rest'], 'sessionId': sid}}
                os.makedirs(os.path.dirname(TOKEN_FILE), exist_ok=True)
                with open(TOKEN_FILE, 'w') as f: f.write(json.dumps(tokens, indent=2) + '\\n')
                break
        time.sleep(2)
    cmd({{'command': 'close-tab', 'sessionId': sid}})
except: pass
"""
    proc = subprocess.Popen(
        ['python3', '-c', script],
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        start_new_session=True,
    )
    proc


# --- Main token acquisition ---

def _tokens_are_valid(data):
    """Return True if at least graph or rest token is not expired."""
    now = int(time.time())
    for key in ('graph', 'rest'):
        entry = data.get(key, {})
        exp = entry.get('expiresOn')
        if entry.get('token') and (exp is None or exp > now):
            return True
    return False


def _navigate_and_wait(sid, url, wait_sec=15):
    """Navigate a tab to url via JS exec and wait for fresh tokens."""
    try:
        # Use exec with chrome_navigate (the /browser endpoint doesn't support navigate directly)
        mcp_browser_cmd({'command': 'exec', 'sessionId': sid,
                         'code': f'chrome_navigate("{url}")', 'timeout': 5})
    except Exception:
        pass
    deadline = time.time() + wait_sec
    time.sleep(4)
    while time.time() < deadline:
        data = extract_from_session(sid)
        if data and _tokens_are_valid(data):
            return data
        time.sleep(2)
    return extract_from_session(sid)


def extract_tokens_from_chrome():
    """
    Extract M365 tokens from Chrome.

    Strategy:
    1. Check for an existing Outlook tab with valid tokens — reuse it (no new tab).
    2. If existing tab has expired/stale tokens — wait on that SAME tab for up to 90 s
       (user may be mid-login). Do NOT open another tab to avoid pile-up.
    3. If no Outlook tab exists at all — open one, wait, then close it.
    """
    auto_opened_sid = None  # track a tab we opened so we can close it on success

    # 1. Try existing tab first — valid tokens → done immediately.
    existing_sid = find_outlook_session()
    if existing_sid:
        try:
            data = extract_from_session(existing_sid)
            if data and _tokens_are_valid(data):
                tokens = load_tokens()
                if data.get('graph') and data.get('graph', {}).get('token'):
                    tokens['graph'] = {**data['graph'], 'sessionId': existing_sid}
                if data.get('rest') and data.get('rest', {}).get('token'):
                    tokens['rest'] = {**data['rest'], 'sessionId': existing_sid}
                save_tokens(tokens)
                return tokens
        except Exception:
            pass

        # 2. Existing tab present but tokens stale/absent — wait on it (user may be
        #    mid-login). Do NOT open a second Outlook tab.
        deadline = time.time() + 90
        time.sleep(3)
        while time.time() < deadline:
            try:
                data = extract_from_session(existing_sid)
                rest_entry = (data or {}).get('rest') or {}
                graph_entry = (data or {}).get('graph') or {}
                now = int(time.time())
                rest_ok = rest_entry.get('token') and rest_entry.get('expiresOn', 0) > now
                graph_ok = graph_entry.get('token') and graph_entry.get('expiresOn', 0) > now
                if rest_ok and graph_ok:
                    tokens = load_tokens()
                    tokens['rest'] = {**rest_entry, 'sessionId': existing_sid}
                    tokens['graph'] = {**graph_entry, 'sessionId': existing_sid}
                    save_tokens(tokens)
                    return tokens
            except Exception:
                pass
            time.sleep(3)
        # Still nothing after 90 s — fall through and open a new tab as last resort.

    # 3. No Outlook tab at all — open one, wait, close it when done.
    try:
        auto_opened_sid = open_outlook_tab()
        deadline = time.time() + 90
        time.sleep(8)  # People page needs a bit longer to load Graph API
        while time.time() < deadline:
            data = extract_from_session(auto_opened_sid)
            rest_entry = (data or {}).get('rest') or {}
            graph_entry = (data or {}).get('graph') or {}
            now = int(time.time())
            rest_ok = rest_entry.get('token') and rest_entry.get('expiresOn', 0) > now
            graph_ok = graph_entry.get('token') and graph_entry.get('expiresOn', 0) > now
            if rest_ok and graph_ok:
                # Both tokens available — save and close immediately.
                tokens = load_tokens()
                tokens['rest'] = {**rest_entry, 'sessionId': None}
                tokens['graph'] = {**graph_entry, 'sessionId': None}
                save_tokens(tokens)
                close_tab(auto_opened_sid)
                return tokens
            if rest_ok and time.time() > deadline - 10:
                # Running out of time — accept rest-only (graph may not appear).
                break
            time.sleep(3)

        # Timed out — read whatever we have before closing.
        data = extract_from_session(auto_opened_sid)
        close_tab(auto_opened_sid)
        if not data or (not data.get('graph') and not data.get('rest')):
            raise RuntimeError('Token extraction returned empty result after 90s')
        tokens = load_tokens()
        if data.get('graph') and data.get('graph', {}).get('token'):
            tokens['graph'] = {**data['graph'], 'sessionId': None}
        if data.get('rest') and data.get('rest', {}).get('token'):
            tokens['rest'] = {**data['rest'], 'sessionId': None}
        save_tokens(tokens)
        return tokens

    except Exception:
        if auto_opened_sid:
            close_tab(auto_opened_sid)
        raise


def refresh_from_session(sid):
    data = extract_from_session(sid)
    if not data:
        return None
    tokens = load_tokens()
    if data.get('graph', {}).get('token'):
        tokens['graph'] = {**data['graph'], 'sessionId': sid}
    if data.get('rest', {}).get('token'):
        tokens['rest'] = {**data['rest'], 'sessionId': sid}
    save_tokens(tokens)
    return tokens


def ensure_token(token_type, force=False):
    tokens = load_tokens()
    entry = tokens.get(token_type)

    if not force and is_token_usable(entry):
        if is_token_expiring_soon(entry) and token_type in ('graph', 'rest'):
            background_refresh()
        return entry['token']

    if token_type == 'teams':
        return ensure_teams_token(force=force)

    sid = (entry or {}).get('sessionId') or find_outlook_session()
    if sid:
        try:
            sessions = mcp_browser_cmd({'command': 'sessions'})
            if sessions.get('ok') and any(s['id'] == sid for s in sessions.get('sessions', [])):
                refreshed = refresh_from_session(sid)
                if refreshed and is_token_usable((refreshed or {}).get(token_type) or {}):
                    return ((refreshed or {}).get(token_type) or {})['token']
        except Exception:
            pass

    extracted = extract_tokens_from_chrome()
    extracted_entry = (extracted or {}).get(token_type) or {}
    if is_token_usable(extracted_entry):
        return extracted_entry['token']

    new_sid, fresh_data = open_and_wait_for_token(token_type)
    if fresh_data:
        t = load_tokens()
        if fresh_data.get('graph') and fresh_data.get('graph', {}).get('token'):
            t['graph'] = {**fresh_data['graph'], 'sessionId': None}
        if fresh_data.get('rest') and fresh_data.get('rest', {}).get('token'):
            t['rest'] = {**fresh_data['rest'], 'sessionId': None}
        save_tokens(t)
        t_entry = (t or {}).get(token_type) or {}
        if is_token_usable(t_entry):
            return t_entry['token']
    else:
        pass  # tab already closed inside open_and_wait_for_token

    raise RuntimeError(f'Could not obtain a valid {token_type} token. Ensure you are logged into Outlook Web.')


def ensure_teams_token(force=False):
    tokens = load_tokens()
    entry = tokens.get('teams')
    if not force and is_token_usable(entry):
        return entry['token']
    try:
        rt = ensure_teams_refresh_token()
        return exchange_refresh_token(rt, '6bc3b958-689b-49f5-9006-36d165f30e00/.default', 'teams')
    except Exception as e:
        after = load_tokens()
        if not after.get('teamsRefreshToken'):
            dlog(f'ensure_teams_token retry after RT revocation — forcing re-auth')
            rt = ensure_teams_refresh_token(force_reauth=True)
            return exchange_refresh_token(rt, '6bc3b958-689b-49f5-9006-36d165f30e00/.default', 'teams')
        raise


def ensure_teams_refresh_token(force_reauth=False):
    tokens = load_tokens()
    if not force_reauth and tokens.get('teamsRefreshToken'):
        return tokens['teamsRefreshToken']

    sid = find_teams_session()
    if sid:
        if force_reauth:
            dlog('ensure_teams_refresh_token navigating existing tab to force re-auth')
            navigate_tab(sid, 'https://teams.microsoft.com/')
            time.sleep(4)
        data = extract_teams_rt_from_session(sid)
        if data and data.get('refreshToken'):
            t = load_tokens()
            t['teamsRefreshToken'] = data['refreshToken']
            t['teamsClientId'] = data.get('clientId')
            if data.get('tenantId'):
                t['teamsTenantId'] = data['tenantId']
            save_tokens(t)
            return data['refreshToken']
        # exec returned null (extension disconnected or wrong tab) — fall through to open new tab

    sid = open_teams_tab()
    deadline = time.time() + 30
    time.sleep(5)
    while time.time() < deadline:
        data = extract_teams_rt_from_session(sid)
        if data and data.get('refreshToken'):
            t = load_tokens()
            t['teamsRefreshToken'] = data['refreshToken']
            t['teamsClientId'] = data.get('clientId')
            if data.get('tenantId'):
                t['teamsTenantId'] = data['tenantId']
            save_tokens(t)
            close_tab(sid)
            return data['refreshToken']
        time.sleep(2)

    close_tab(sid)
    raise RuntimeError('Could not extract Teams refresh token. Ensure you are logged into Teams Web.')


def exchange_refresh_token(refresh_token, scope, cache_key=None):
    tokens = load_tokens()
    client_id = tokens.get('teamsClientId') or '5e3ce6c0-2b1f-4285-8d4b-75ee78787346'
    tenant_id = get_tenant_id(tokens)

    url = f'https://login.microsoftonline.com/{tenant_id}/oauth2/v2.0/token'
    body = '&'.join([
        f"client_id={urllib.parse.quote(client_id, safe='')}",
        f"redirect_uri={urllib.parse.quote('https://teams.microsoft.com/v2/auth', safe='')}",
        f"scope={urllib.parse.quote(scope + ' openid profile offline_access', safe='')}",
        'grant_type=refresh_token',
        f"refresh_token={urllib.parse.quote(refresh_token, safe='')}",
    ])

    result = subprocess.run(
        ['curl', '-s', url,
         '-H', 'content-type: application/x-www-form-urlencoded;charset=utf-8',
         '-H', 'origin: https://teams.microsoft.com',
         '--data-raw', body],
        capture_output=True, text=True, timeout=15,
    )

    try:
        token_data = json.loads(result.stdout)
    except Exception:
        raise RuntimeError('Token exchange returned invalid JSON')

    if token_data.get('error'):
        t = load_tokens()
        t.pop('teamsRefreshToken', None)
        save_tokens(t)
        raise RuntimeError(f"Token exchange failed: {token_data.get('error_description') or token_data['error']}. Retry to re-extract.")

    now_sec = int(time.time())
    expires_on = now_sec + (token_data.get('expires_in') or 3600)

    if cache_key:
        t = load_tokens()
        t[cache_key] = {'token': token_data['access_token'], 'expiresOn': expires_on}
        if token_data.get('refresh_token'):
            t['teamsRefreshToken'] = token_data['refresh_token']
        save_tokens(t)

    return token_data['access_token']


def ensure_spo_token(spo_host):
    cache_key = f'spo:{spo_host}'
    tokens = load_tokens()
    entry = tokens.get(cache_key)
    if is_token_usable(entry):
        return entry['token']
    try:
        rt = ensure_teams_refresh_token()
        return exchange_refresh_token(rt, f'https://{spo_host}/.default', cache_key)
    except Exception as e:
        after = load_tokens()
        if not after.get('teamsRefreshToken'):
            dlog(f'ensure_spo_token {spo_host} retry after RT revocation — forcing re-auth')
            rt = ensure_teams_refresh_token(force_reauth=True)
            return exchange_refresh_token(rt, f'https://{spo_host}/.default', cache_key)
        raise


def ensure_substrate_token():
    tokens = load_tokens()
    entry = tokens.get('substrate')
    if is_token_usable(entry):
        return entry['token']
    try:
        rt = ensure_teams_refresh_token()
        return exchange_refresh_token(rt, 'https://substrate.office.com/.default', 'substrate')
    except Exception as e:
        after = load_tokens()
        if not after.get('teamsRefreshToken'):
            dlog('ensure_substrate_token retry after RT revocation — forcing re-auth')
            rt = ensure_teams_refresh_token(force_reauth=True)
            return exchange_refresh_token(rt, 'https://substrate.office.com/.default', 'substrate')
        raise


def ensure_csa_token():
    tokens = load_tokens()
    entry = tokens.get('csa')
    if is_token_usable(entry):
        return entry['token']
    try:
        rt = ensure_teams_refresh_token()
        return exchange_refresh_token(rt, 'https://chatsvcagg.teams.microsoft.com/.default', 'csa')
    except Exception as e:
        after = load_tokens()
        if not after.get('teamsRefreshToken'):
            dlog('ensure_csa_token retry after RT revocation — forcing re-auth')
            rt = ensure_teams_refresh_token(force_reauth=True)
            return exchange_refresh_token(rt, 'https://chatsvcagg.teams.microsoft.com/.default', 'csa')
        raise


def ensure_graph_token(force=False):
    """Get a Graph API token by exchanging the Teams refresh token."""
    tokens = load_tokens()
    entry = tokens.get('graph')
    if not force and is_token_usable(entry):
        return entry['token']
    try:
        rt = ensure_teams_refresh_token()
        return exchange_refresh_token(rt, 'https://graph.microsoft.com/.default', 'graph')
    except Exception:
        after = load_tokens()
        if not after.get('teamsRefreshToken'):
            dlog('ensure_graph_token retry after RT revocation — forcing re-auth')
            rt = ensure_teams_refresh_token(force_reauth=True)
            return exchange_refresh_token(rt, 'https://graph.microsoft.com/.default', 'graph')
        raise


def ensure_chat_graph_token(force=False):
    """Get a Graph API token with Chat.Read scope.

    The token extracted directly from the Outlook tab (appid 9199bf20, Outlook
    Web Client) includes Chat.Read scope, unlike the token obtained via Teams
    refresh token exchange (appid 5e3ce6c0, Teams Web Client) which does not.

    We cache it under the 'graph_chat' key to avoid overwriting the regular
    graph token used for mail/calendar calls.
    """
    tokens = load_tokens()
    entry = tokens.get('graph_chat')
    if not force and is_token_usable(entry):
        return entry['token']

    # Extract fresh token from the open Outlook tab.
    sid = find_outlook_session()
    if not sid:
        raise RuntimeError(
            'No Outlook tab found. Please open Outlook Web to enable Teams chat sync.'
        )
    data = extract_from_session(sid)
    now = int(time.time())
    graph_entry = (data or {}).get('graph') or {}
    if graph_entry.get('token') and graph_entry.get('expiresOn', 0) > now:
        t = load_tokens()
        t['graph_chat'] = {'token': graph_entry['token'], 'expiresOn': graph_entry['expiresOn']}
        save_tokens(t)
        return graph_entry['token']

    raise RuntimeError(
        'Could not obtain a Chat.Read graph token from Outlook tab. '
        'Ensure Outlook Web is open and you are logged in.'
    )


def get_tenant_id(tokens):
    if tokens.get('teamsTenantId'):
        return tokens['teamsTenantId']
    for key in ('graph', 'teams'):
        t = (tokens.get(key) or {}).get('token')
        if not t:
            continue
        try:
            payload_b64 = t.split('.')[1]
            # Add padding
            payload_b64 += '=' * (-len(payload_b64) % 4)
            payload = json.loads(base64.b64decode(payload_b64).decode('utf-8'))
            if payload.get('tid'):
                return payload['tid']
        except Exception:
            pass
    raise RuntimeError('Tenant ID not found. Run "m365-cli auth login" or ensure Teams Web is open.')


# --- SAP additional services ---

def get_aha_token():
    """Return the Aha! API token, or None."""
    return load_credentials().get('ahaToken') or None


def set_aha_token(token_str):
    """Persist an Aha! API token."""
    creds = load_credentials()
    creds['ahaToken'] = token_str
    save_credentials(creds)


def clear_aha_token():
    creds = load_credentials()
    creds.pop('ahaToken', None)
    save_credentials(creds)


# --- SAP GitHub PAT tokens ---

def get_github_token(host_key):
    """Return the stored GitHub PAT for the given host key (e.g. 'tools' or 'wdf'), or None."""
    return load_credentials().get(f'githubToken:{host_key}') or None


def set_github_token(host_key, token_str):
    """Persist a GitHub PAT for the given host key."""
    creds = load_credentials()
    creds[f'githubToken:{host_key}'] = token_str
    save_credentials(creds)


def clear_github_token(host_key):
    creds = load_credentials()
    creds.pop(f'githubToken:{host_key}', None)
    save_credentials(creds)


def check_github_accessible(host_key, api_base):
    """Return True if the stored PAT can reach the GitHub API /user endpoint."""
    pat = get_github_token(host_key)
    if not pat:
        return False
    try:
        import urllib.request
        req = urllib.request.Request(
            f'{api_base}/api/v3/user',
            headers={'Authorization': f'token {pat}', 'User-Agent': 'openhuman/1.0'},
        )
        resp = urllib.request.urlopen(req, timeout=8)
        return resp.status == 200
    except Exception:
        return False


def check_sso_accessible(url, success_check=None):
    """Check whether mcp-chrome can fetch the given URL successfully.
    Uses the /browser fetch command which carries Chrome's cookies automatically.
    success_check: optional callable(response_text) -> bool for extra validation.
    """
    try:
        resp = mcp_browser_cmd({'command': 'fetch', 'url': url}, timeout_ms=10000)
        if not resp.get('ok'):
            return False
        body = resp.get('data') or resp.get('body') or ''
        if isinstance(body, bytes):
            body = body.decode('utf-8', errors='ignore')
        if success_check:
            return success_check(str(body))
        return bool(body) and 'error' not in str(body).lower()[:100]
    except Exception:
        return False


def check_domain_cookies(domain):
    """Check whether Chrome has cookies for a given domain (SSO session indicator)."""
    try:
        resp = mcp_browser_cmd({'command': 'get-cookies', 'domain': domain}, timeout_ms=2000)
        if not resp.get('ok'):
            return False
        # get-cookies response has 'cookieHeader' or 'data' or 'cookies' key
        cookie_str = resp.get('cookieHeader') or resp.get('data') or ''
        cookies_list = resp.get('cookies') or []
        return bool(cookie_str and cookie_str.strip()) or bool(cookies_list)
    except Exception:
        return False


def check_jira_accessible():
    """Check whether SAP Jira is accessible — uses cookie presence as indicator."""
    return check_domain_cookies('jira.tools.sap')


def check_wiki_accessible():
    """Check whether SAP Confluence Wiki is accessible — uses cookie presence."""
    return check_domain_cookies('wiki.one.int.sap')


def check_sso_session(domain):
    """Kept for backward compat — checks for an open Chrome tab on the domain."""
    try:
        resp = mcp_browser_cmd({'command': 'sessions'})
        if not resp.get('ok'):
            return False
        return any(domain in (s.get('url') or '') for s in resp.get('sessions', []))
    except Exception:
        return False


def ensure_spo_token_cached():
    """Ensure sap.sharepoint.com SPO token is cached and valid."""
    return ensure_spo_token('sap.sharepoint.com')
