#!/usr/bin/env python3
import json
import os
import re
import sys
from urllib import request as urllib_request
from urllib.error import URLError

DEFAULT_PORT = int(os.environ.get('MCP_CHROME_PORT') or os.environ.get('CHROME_MCP_PORT') or 12306)

# Commands routed to /browser (extension background, no CORS)
# 'exec' routes to /mcp chrome_javascript for arbitrary JS execution
BROWSER_COMMANDS = {'fetch', 'sessions', 'new-tab', 'close-tab', 'get-cookies'}

# --- Arg parsing ---

def parse_args(argv):
    args = argv[1:]  # skip script name
    opts = {'command': None, 'tool': None, 'tool_args': {}, 'port': DEFAULT_PORT, 'raw': False}

    i = 0
    while i < len(args):
        a = args[i]
        if a in ('list', 'help', 'call'):
            opts['command'] = a
            if a in ('help', 'call'):
                i += 1
                opts['tool'] = args[i] if i < len(args) else None
        elif a in BROWSER_COMMANDS or a == 'exec':
            opts['command'] = a
        elif a == '--raw':
            opts['raw'] = True
        elif a == '--port' and i + 1 < len(args):
            i += 1
            opts['port'] = int(args[i])
        elif a == '--arg' and i + 1 < len(args):
            i += 1
            kv = args[i]
            eq = kv.find('=')
            if eq == -1:
                opts['tool_args'][kv] = True
            else:
                opts['tool_args'][kv[:eq]] = try_parse_json(kv[eq + 1:])
        elif a == '--json' and i + 1 < len(args):
            i += 1
            try:
                opts['tool_args'].update(json.loads(args[i]))
            except json.JSONDecodeError:
                die(f'--json value is not valid JSON: {args[i]}')
        i += 1

    return opts


def try_parse_json(s):
    trimmed = s.strip()
    if (trimmed.startswith('{') or trimmed.startswith('[')
            or trimmed in ('true', 'false', 'null')
            or bool(re.match(r'^-?\d', trimmed))):
        try:
            return json.loads(trimmed)
        except json.JSONDecodeError:
            pass
    return s


def die(msg):
    print(f'Error: {msg}', file=sys.stderr)
    sys.exit(1)


# --- /mcp route client ---

def _http_post(url, headers, body_str):
    data = body_str.encode('utf-8')
    req = urllib_request.Request(url, data=data, headers=headers, method='POST')
    try:
        with urllib_request.urlopen(req) as resp:
            return resp.headers, resp.read().decode('utf-8')
    except URLError as e:
        port = url.split(':')[2].split('/')[0]
        die(f'Cannot reach mcp-chrome server on port {port}: {e}\nIs Chrome open with the extension connected?')


def mcp_post(port, session_id, body):
    headers = {
        'Content-Type': 'application/json',
        'Accept': 'application/json, text/event-stream',
    }
    if session_id:
        headers['mcp-session-id'] = session_id

    resp_headers, text = _http_post(f'http://127.0.0.1:{port}/mcp', headers, json.dumps(body))

    new_session = resp_headers.get('mcp-session-id')
    ct = resp_headers.get('Content-Type', '')

    if 'text/event-stream' in ct:
        parsed = _parse_sse(text)
    else:
        try:
            parsed = json.loads(text)
        except json.JSONDecodeError:
            parsed = {'raw': text}

    return parsed, new_session or session_id


def _parse_sse(text):
    last = None
    for line in text.split('\n'):
        if line.startswith('data: '):
            data = line[6:].strip()
            if not data:
                continue
            try:
                last = json.loads(data)
            except json.JSONDecodeError:
                pass
    return last


def init_session(port):
    _, session_id = mcp_post(port, None, {
        'jsonrpc': '2.0',
        'id': 1,
        'method': 'initialize',
        'params': {
            'protocolVersion': '2024-11-05',
            'capabilities': {},
            'clientInfo': {'name': 'mcp-cli', 'version': '1.0'},
        },
    })
    if not session_id:
        die('Server did not return mcp-session-id. Is the server running?')
    return session_id


def close_session(port, session_id):
    """Best-effort DELETE /mcp to release the server-side transport.
    The bridge holds a single MCP session at a time and rejects re-init
    until the previous session is closed (HTTP 500 'Already connected to
    a transport'); without this every second CLI call wedges."""
    if not session_id:
        return
    req = urllib_request.Request(
        f'http://127.0.0.1:{port}/mcp',
        headers={'mcp-session-id': session_id},
        method='DELETE',
    )
    try:
        urllib_request.urlopen(req, timeout=2).read()
    except Exception:
        pass


def list_tools(port, session_id):
    parsed, _ = mcp_post(port, session_id, {'jsonrpc': '2.0', 'id': 2, 'method': 'tools/list'})
    if parsed and parsed.get('error'):
        die(f'tools/list error: {json.dumps(parsed["error"])}')
    return (parsed or {}).get('result', {}).get('tools', [])


def call_tool(port, session_id, name, tool_args):
    parsed, _ = mcp_post(port, session_id, {
        'jsonrpc': '2.0',
        'id': 3,
        'method': 'tools/call',
        'params': {'name': name, 'arguments': tool_args},
    })
    return parsed


# --- /browser route client (extension background, no CORS) ---

def browser_post(port, body):
    _, text = _http_post(
        f'http://127.0.0.1:{port}/browser',
        {'Content-Type': 'application/json'},
        json.dumps(body),
    )
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        die('Invalid JSON response from /browser endpoint')


# --- Output formatting ---

def print_tools(tools):
    name_width = max((len(t['name']) for t in tools), default=10)
    for t in tools:
        desc = (t.get('description') or '').split('\n')[0]
        print(f"  {t['name'].ljust(name_width)}  {desc}")


def print_schema(tool):
    print(f"Tool: {tool['name']}")
    print(f"Description: {tool.get('description') or '(none)'}")
    props = (tool.get('inputSchema') or {}).get('properties') or {}
    required = set((tool.get('inputSchema') or {}).get('required') or [])
    if not props:
        print('Arguments: (none)')
        return
    print('Arguments:')
    for key, schema in props.items():
        req = ' (required)' if key in required else ''
        typ = schema.get('type')
        if isinstance(typ, list):
            typ = f" [{'|'.join(typ)}]"
        elif typ:
            typ = f' [{typ}]'
        else:
            typ = ''
        desc = schema.get('description', '')
        desc = f"  — {desc.split(chr(10))[0]}" if desc else ''
        print(f'  --arg {key}=<value>{typ}{req}{desc}')


def print_result(parsed):
    if parsed is None:
        print('(no response)')
        return
    if parsed.get('error'):
        err = parsed['error']
        print(f"Error: {err.get('message') or json.dumps(err)}", file=sys.stderr)
        sys.exit(1)
    content = (parsed.get('result') or {}).get('content')
    if not content:
        print(json.dumps(parsed.get('result', parsed), indent=2))
        return
    for item in content:
        if item.get('type') == 'text':
            print(item['text'])
        elif item.get('type') == 'image':
            mime = item.get('mimeType', 'unknown')
            size = len(item.get('data') or '')
            print(f'[image: {mime}, {size} bytes base64]')
        else:
            print(json.dumps(item, indent=2))


def print_browser_result(result, raw):
    if raw:
        print(json.dumps(result, indent=2))
        return
    if not result.get('ok'):
        print(f"Error: {result.get('error') or json.dumps(result)}", file=sys.stderr)
        sys.exit(1)
    if 'sessions' in result:
        for s in result['sessions']:
            print(f"{str(s['id']).ljust(12)}  {s.get('title') or '(no title)'}")
            print(f"{''.ljust(12)}  {s.get('url', '')}")
        return
    data = result.get('data')
    if isinstance(data, dict) and data.get('sessionId') is not None:
        print(json.dumps(data))
        return
    if result.get('cookieHeader') is not None:
        print(result['cookieHeader'])
        return
    if result.get('statusCode') is not None:
        print(f"HTTP {result['statusCode']}")
        body = result.get('body')
        print(body if isinstance(body, str) else json.dumps(body, indent=2))
        return
    if data is not None:
        print(data if isinstance(data, str) else json.dumps(data, indent=2))
        return
    print(json.dumps(result, indent=2))


def print_usage():
    print(f"""\
Usage:
  mcp-chrome-cli <command> [options]

━━━ /mcp route — content script in active tab, CORS applies ━━━

  list                                    List all available MCP tools
  help <tool>                             Show tool arguments and description
  call <tool> [--arg key=value ...]       Call an MCP tool
  exec --arg tabId=N --arg code='...'     Execute arbitrary JS in a tab (alias for chrome_javascript)

━━━ /browser route — extension background, no CORS, no active-tab required ━━━

  fetch --arg url=URL                     HTTP request via extension background (no CORS)
    [--arg method=POST]                     HTTP method (default: GET)
    [--arg headers='{{"K":"V"}}']             Request headers as JSON object
    [--arg body='...']                      Request body
    [--arg timeout=30]                      Timeout in seconds (default: 30)

  sessions                                List all open tabs (tabId + title + url)
  new-tab --arg url=URL                   Open a new tab, returns tabId
  close-tab --arg tabId=N                 Close a tab by tabId
  get-cookies --arg domain=DOMAIN         Get cookies for a domain, returns Cookie header string

━━━ Options ━━━

  --arg key=value                         Add an argument (repeatable)
                                          Value auto-parsed as JSON if it looks like JSON
  --json '{{"key":"value"}}'                Pass all arguments as a JSON object
  --port <number>                         Server port (default: {DEFAULT_PORT})
  --raw                                   Print raw JSON response

Examples:
  mcp-chrome-cli list
  mcp-chrome-cli help chrome_navigate
  mcp-chrome-cli call chrome_javascript --arg tabId=123 --arg code='return document.title'
  mcp-chrome-cli exec --arg tabId=123 --arg code='return document.title'
  mcp-chrome-cli fetch --arg url=https://jira.tools.sap/rest/api/2/issue/PROJ-1
  mcp-chrome-cli sessions
  mcp-chrome-cli new-tab --arg url=https://example.com
  mcp-chrome-cli get-cookies --arg domain=jira.tools.sap
""")


# --- Main ---

def main():
    opts = parse_args(sys.argv)

    if not opts['command']:
        print_usage()
        sys.exit(0)

    # /browser route — no MCP session needed
    if opts['command'] in BROWSER_COMMANDS:
        a = opts['tool_args']
        cmd = opts['command']

        if cmd == 'sessions':
            body = {'command': 'sessions'}
        elif cmd == 'new-tab':
            if not a.get('url'):
                die('new-tab requires --arg url=URL')
            body = {'command': 'new-tab', 'url': a['url']}
        elif cmd == 'close-tab':
            if a.get('tabId') is None:
                die('close-tab requires --arg tabId=N')
            body = {'command': 'close-tab', 'sessionId': int(a['tabId'])}
        elif cmd == 'get-cookies':
            if not a.get('domain'):
                die('get-cookies requires --arg domain=DOMAIN')
            raw_domain = re.sub(r'^https?://', '', str(a['domain'])).split('/')[0]
            body = {'command': 'get-cookies', 'domain': raw_domain}
        elif cmd == 'fetch':
            if not a.get('url'):
                die('fetch requires --arg url=URL')
            body = {'command': 'fetch', 'url': a['url'], 'method': a.get('method', 'GET')}
            if a.get('headers') is not None:
                body['headers'] = a['headers']
            if a.get('body') is not None:
                body['body'] = a['body']
            if a.get('timeout') is not None:
                body['timeout'] = int(a['timeout'])

        result = browser_post(opts['port'], body)
        print_browser_result(result, opts['raw'])
        return

    # /mcp route — need session
    session_id = init_session(opts['port'])

    try:
        if opts['command'] == 'exec':
            a = opts['tool_args']
            if a.get('tabId') is None:
                die("exec requires --arg tabId=N")
            if a.get('code') is None:
                die("exec requires --arg code='...'")
            parsed = call_tool(opts['port'], session_id, 'chrome_javascript', {
                'tabId': int(a['tabId']),
                'code': str(a['code']),
            })
            if opts['raw']:
                print(json.dumps(parsed, indent=2))
            else:
                print_result(parsed)
            return

        if opts['command'] == 'list':
            tools = list_tools(opts['port'], session_id)
            print(f'{len(tools)} tools available:\n')
            print_tools(tools)
            return

        if opts['command'] == 'help':
            if not opts['tool']:
                die('Usage: mcp-chrome-cli help <tool-name>')
            tools = list_tools(opts['port'], session_id)
            tool = next((t for t in tools if t['name'] == opts['tool']), None)
            if not tool:
                die(f"Unknown tool: {opts['tool']}\nRun \"mcp-chrome-cli list\" to see available tools.")
            print_schema(tool)
            return

        if opts['command'] == 'call':
            if not opts['tool']:
                die('Usage: mcp-chrome-cli call <tool-name> [--arg key=value ...]')
            parsed = call_tool(opts['port'], session_id, opts['tool'], opts['tool_args'])
            if opts['raw']:
                print(json.dumps(parsed, indent=2))
            else:
                print_result(parsed)
            return
    finally:
        close_session(opts['port'], session_id)


if __name__ == '__main__':
    try:
        main()
    except KeyboardInterrupt:
        sys.exit(130)
    except Exception as e:
        print(str(e), file=sys.stderr)
        sys.exit(1)
