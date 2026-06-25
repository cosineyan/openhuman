import json
import time
import urllib.parse
import urllib.request
import urllib.error

from .tokens import ensure_token, ensure_csa_token

GRAPH_BASE = 'https://graph.microsoft.com/v1.0'
GRAPH_BETA = 'https://graph.microsoft.com/beta'
REST_BASE = 'https://outlook.office.com/api/v2.0'
CSA_BASE = 'https://teams.microsoft.com/api/csa'


def api_request(base_url, endpoint, method='GET', headers=None, body=None,
                token=None, timeout=30, raw=False):
    if endpoint.startswith('https://'):
        url = endpoint
    elif raw:
        url = f'{base_url}/{endpoint}'
    else:
        url = f'{base_url}/me/{endpoint}'
    url = urllib.parse.quote(url, safe=':/?&=$%+@,!~*\'()')

    req_headers = {
        'Authorization': f'Bearer {token}',
        'Content-Type': 'application/json',
    }
    if headers:
        extra = json.loads(headers) if isinstance(headers, str) else headers
        req_headers.update(extra)

    data = None
    if body and method != 'GET':
        data = (json.dumps(body) if not isinstance(body, str) else body).encode('utf-8')

    req = urllib.request.Request(url, data=data, headers=req_headers, method=method)

    start = time.time()
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            content_type = resp.headers.get('Content-Type', '')
            raw_body = resp.read().decode('utf-8')
            resp_body = json.loads(raw_body) if 'application/json' in content_type else raw_body
            return {
                'status_code': resp.status,
                'headers': dict(resp.headers),
                'body': resp_body,
                'elapsed_ms': int((time.time() - start) * 1000),
            }
    except urllib.error.HTTPError as e:
        content_type = e.headers.get('Content-Type', '')
        raw_body = e.read().decode('utf-8')
        resp_body = json.loads(raw_body) if 'application/json' in content_type else raw_body
        return {
            'status_code': e.code,
            'headers': dict(e.headers),
            'body': resp_body,
            'elapsed_ms': int((time.time() - start) * 1000),
        }


def graph_request(endpoint, method='GET', headers=None, body=None,
                  timeout=30, raw=False, token=None, beta=False, _retried=False):
    tok = token or ensure_token('graph')
    base = GRAPH_BETA if beta else GRAPH_BASE
    result = api_request(base, endpoint, method=method, headers=headers,
                         body=body, token=tok, timeout=timeout, raw=raw)
    if result['status_code'] == 401 and not _retried:
        fresh = ensure_token('graph', force=True)
        return graph_request(endpoint, method=method, headers=headers, body=body,
                             timeout=timeout, raw=raw, token=fresh, beta=beta, _retried=True)
    return result


def rest_request(endpoint, method='GET', headers=None, body=None,
                 timeout=30, raw=False, token=None, _retried=False):
    tok = token or ensure_token('rest')
    result = api_request(REST_BASE, endpoint, method=method, headers=headers,
                         body=body, token=tok, timeout=timeout, raw=raw)
    if result['status_code'] == 401 and not _retried:
        fresh = ensure_token('rest', force=True)
        return rest_request(endpoint, method=method, headers=headers, body=body,
                            timeout=timeout, raw=raw, token=fresh, _retried=True)
    return result


def csa_request(region, endpoint, method='GET', headers=None, body=None,
                timeout=30, token=None, _retried=False):
    tok = token or ensure_csa_token()
    csa_headers = {
        'x-ms-client-version': '1415/26021215123',
        'x-ms-user-partition': f'{region}01',
        'x-ms-region': region,
        'x-ms-client-type': 'cdlworker',
        **(headers or {}),
    }
    url = f'{CSA_BASE}/{region}/api/v1'
    result = api_request(url, endpoint, method=method, headers=csa_headers,
                         body=body, token=tok, timeout=timeout, raw=True)
    if result['status_code'] == 401 and not _retried:
        fresh = ensure_csa_token()
        return csa_request(region, endpoint, method=method, headers=headers,
                           body=body, timeout=timeout, token=fresh, _retried=True)
    return result
