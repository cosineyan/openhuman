import json
import click
from ..api import graph_request


@click.command('graph')
@click.argument('endpoint')
@click.option('-m', '--method', default='GET', help='HTTP method')
@click.option('-H', '--headers', 'hdr', default=None, help='Extra headers as JSON')
@click.option('-b', '--body', default=None, help='Request body as JSON')
@click.option('-t', '--timeout', default='30', help='Request timeout in seconds')
@click.option('--raw', is_flag=True, help='Use endpoint as-is (skip /me/ prefix)')
@click.option('--beta', is_flag=True, help='Use beta API (graph.microsoft.com/beta)')
@click.pass_context
def graph_cmd(ctx, endpoint, method, hdr, body, timeout, raw, beta):
    """Call Microsoft Graph API. Pass a full https:// URL as endpoint to use it directly (e.g. a nextLink or deltaLink)."""
    try:
        result = graph_request(
            endpoint,
            method=method,
            headers=json.loads(hdr) if hdr else {},
            body=json.loads(body) if body else None,
            timeout=int(timeout),
            raw=raw,
            beta=beta,
        )
        ctx.obj['out']({'ok': True, **result})
    except Exception as e:
        ctx.obj['die'](str(e))
