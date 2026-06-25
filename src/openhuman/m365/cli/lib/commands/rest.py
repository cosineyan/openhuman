import json
import click
from ..api import rest_request


@click.command('rest')
@click.argument('endpoint')
@click.option('-m', '--method', default='GET', help='HTTP method')
@click.option('-H', '--headers', 'hdr', default=None, help='Extra headers as JSON')
@click.option('-b', '--body', default=None, help='Request body as JSON')
@click.option('-t', '--timeout', default='30', help='Request timeout in seconds')
@click.option('--raw', is_flag=True, help='Use endpoint as-is (skip /me/ prefix)')
@click.pass_context
def rest_cmd(ctx, endpoint, method, hdr, body, timeout, raw):
    """Call Outlook REST API v2.0 (https://outlook.office.com/api/v2.0/me/<endpoint>)."""
    try:
        result = rest_request(
            endpoint,
            method=method,
            headers=json.loads(hdr) if hdr else {},
            body=json.loads(body) if body else None,
            timeout=int(timeout),
            raw=raw,
        )
        ctx.obj['out']({'ok': True, **result})
    except Exception as e:
        ctx.obj['die'](str(e))
