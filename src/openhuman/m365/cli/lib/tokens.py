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
        if 'outlook.office.com' in url or 'outlook.cloud.microsoft' in url:
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
        if 'teams.microsoft.com' in s.get('url', ''):
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


def extract_from_session(sid):
    resp = mcp_browser_cmd({'command': 'exec', 'sessionId': sid, 'code': EXTRACT_JS, 'timeout': 15}, 30000)
    if not resp.get('ok') or resp.get('data') is None:
        return None
    data = resp['data']
    return json.loads(data) if isinstance(data, str) else data


def open_and_wait_for_token(token_type, max_wait_sec=90):
    sid = open_outlook_tab()
    deadline = time.time() + max_wait_sec
    time.sleep(3)
    while time.time() < deadline:
        data = extract_from_session(sid)
        if data and data.get(token_type, {}).get('token'):
            if data[token_type].get('expiresOn', 0) > int(time.time()):
                return sid, data
        time.sleep(2)
    data = extract_from_session(sid)
    return sid, data


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
    sid = find_outlook_session()
    if sid:
        try:
            data = extract_from_session(sid)
            if data and (data.get('graph') or data.get('rest')):
                # If tokens are expired, navigate the tab to refresh them.
                if not _tokens_are_valid(data):
                    data = _navigate_and_wait(sid, 'https://outlook.office.com/mail/')
                if data:
                    tokens = load_tokens()
                    if data.get('graph', {}).get('token'):
                        tokens['graph'] = {**data['graph'], 'sessionId': sid}
                    if data.get('rest', {}).get('token'):
                        tokens['rest'] = {**data['rest'], 'sessionId': sid}
                    save_tokens(tokens)
                    return tokens
        except Exception:
            pass

    sid = open_outlook_tab()
    time.sleep(15)
    data = extract_from_session(sid)
    if not data or (not data.get('graph') and not data.get('rest')):
        close_tab(sid)
        raise RuntimeError('Token extraction returned empty result')

    tokens = load_tokens()
    if data.get('graph', {}).get('token'):
        tokens['graph'] = {**data['graph'], 'sessionId': None}
    if data.get('rest', {}).get('token'):
        tokens['rest'] = {**data['rest'], 'sessionId': None}
    save_tokens(tokens)
    close_tab(sid)
    return tokens


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
                if refreshed and is_token_usable(refreshed.get(token_type)):
                    return refreshed[token_type]['token']
        except Exception:
            pass

    extracted = extract_tokens_from_chrome()
    if is_token_usable(extracted.get(token_type)):
        return extracted[token_type]['token']

    new_sid, fresh_data = open_and_wait_for_token(token_type)
    if fresh_data:
        t = load_tokens()
        if fresh_data.get('graph', {}).get('token'):
            t['graph'] = {**fresh_data['graph'], 'sessionId': None}
        if fresh_data.get('rest', {}).get('token'):
            t['rest'] = {**fresh_data['rest'], 'sessionId': None}
        save_tokens(t)
        close_tab(new_sid)
        if is_token_usable(t.get(token_type)):
            return t[token_type]['token']
    else:
        close_tab(new_sid)

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
