import urllib.parse
import click
from ..api import graph_request

USER_ALL_SELECT = 'aboutMe,accountEnabled,ageGroup,assignedLicenses,assignedPlans,birthday,businessPhones,city,companyName,consentProvidedForMinor,country,createdDateTime,creationType,department,displayName,employeeHireDate,employeeId,employeeOrgData,employeeType,externalUserState,faxNumber,givenName,hireDate,id,identities,imAddresses,interests,isResourceAccount,jobTitle,lastPasswordChangeDateTime,legalAgeGroupClassification,mail,mailNickname,mobilePhone,mySite,officeLocation,onPremisesDistinguishedName,onPremisesDomainName,onPremisesExtensionAttributes,onPremisesImmutableId,onPremisesLastSyncDateTime,onPremisesProvisioningErrors,onPremisesSamAccountName,onPremisesSecurityIdentifier,onPremisesSyncEnabled,onPremisesUserPrincipalName,otherMails,passwordPolicies,pastProjects,postalCode,preferredLanguage,preferredName,provisionedPlans,proxyAddresses,responsibilities,schools,showInAddressList,signInSessionsValidFromDateTime,skills,state,streetAddress,surname,usageLocation,userPrincipalName,userType'


def fmt_user(u, text_fn):
    parts = [p for p in [
        u.get('displayName'),
        f"({u['mailNickname']})" if u.get('mailNickname') else None,
        f"<{u['mail']}>" if u.get('mail') else None,
        u.get('officeLocation'),
        u.get('jobTitle'),
    ] if p]
    text_fn(' | '.join(parts))
    if u.get('businessPhones'):
        text_fn(f"Phone: {', '.join(u['businessPhones'])}")
    if u.get('mobilePhone'):
        text_fn(f"Mobile: {u['mobilePhone']}")
    if u.get('id'):
        text_fn(f"ID: {u['id']}")


def fmt_time(iso):
    if not iso:
        return ''
    return iso.replace('T', ' ')[:16]


@click.group('me')
def me_cmd():
    """Get current user profile from Microsoft Graph."""


@me_cmd.result_callback()
def me_default(result, **kwargs):
    pass


# Override default action to show profile when no subcommand
@click.pass_context
def me_root(ctx, select, as_json):
    try:
        if select:
            query = f'?$select={select}'
        elif as_json:
            query = f'?$select={USER_ALL_SELECT}'
        else:
            query = ''
        result = graph_request(f'me{query}', raw=True)
        if result['status_code'] != 200:
            ctx.obj['out']({'ok': False, 'status_code': result['status_code'], 'error': result['body']})
            return
        if as_json:
            ctx.obj['out']({'ok': True, 'data': result['body']})
            return
        u = result['body']
        parts = [p for p in [u.get('displayName'), f"({u['mailNickname']})" if u.get('mailNickname') else None, f"<{u['mail']}>" if u.get('mail') else None, u.get('officeLocation'), u.get('jobTitle')] if p]
        ctx.obj['text'](' | '.join(parts))
        if u.get('businessPhones'):
            ctx.obj['text'](f"Phone: {', '.join(u['businessPhones'])}")
        if u.get('mobilePhone'):
            ctx.obj['text'](f"Mobile: {u['mobilePhone']}")
        if u.get('id'):
            ctx.obj['text'](f"ID: {u['id']}")
    except Exception as e:
        ctx.obj['die'](str(e))


# Patch: make "me" callable directly (no subcommand needed)
_original_me_invoke = me_cmd.invoke


def _me_invoke(ctx):
    if ctx.invoked_subcommand is None:
        select = ctx.params.get('select')
        as_json = ctx.params.get('as_json', False)
        with ctx:
            me_root(select=select, as_json=as_json)
    else:
        _original_me_invoke(ctx)


me_cmd.params.append(click.Option(['-s', '--select'], default=None, help='Comma-separated fields'))
me_cmd.params.append(click.Option(['--json', 'as_json'], is_flag=True, help='Output raw JSON'))
me_cmd.invoke = _me_invoke


@me_cmd.group('insights')
def insights():
    """Office Graph insights (recently used files, shared content)."""


@insights.command('used')
@click.option('-n', '--top', default='20', help='Number of results')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def insights_used(ctx, top, as_json):
    """Recently used files and documents."""
    try:
        result = graph_request(f'insights/used?$top={int(top)}')
        if result['status_code'] != 200:
            return ctx.obj['die'](f"Graph API error: {result['status_code']} {result['body']}")
        items = result['body'].get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': items})
        if not items:
            return ctx.obj['text']('No recently used items.')
        ctx.obj['text'](f"--- Recently Used ({len(items)}) ---\n")
        for i, item in enumerate(items):
            vis = item.get('resourceVisualization') or {}
            used = item.get('lastUsed') or {}
            accessed = fmt_time(used.get('lastAccessedDateTime'))
            modified = fmt_time(used.get('lastModifiedDateTime'))
            title = vis.get('title') or '(untitled)'
            item_type = vis.get('type') or ''
            container = vis.get('containerDisplayName') or ''
            ctx.obj['text'](f"{str(i + 1).rjust(2)}. [{accessed}] {title}")
            details = [item_type]
            if modified:
                details.append(f'Modified: {modified}')
            if container:
                details.append(f'Site: {container}')
            ctx.obj['text'](f"    {'  |  '.join(details)}")
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@insights.command('shared')
@click.option('-n', '--top', default='20', help='Number of results')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def insights_shared(ctx, top, as_json):
    """Files and content shared with you."""
    try:
        result = graph_request(f'insights/shared?$top={int(top)}')
        if result['status_code'] != 200:
            return ctx.obj['die'](f"Graph API error: {result['status_code']} {result['body']}")
        items = result['body'].get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': items})
        if not items:
            return ctx.obj['text']('No shared items.')
        ctx.obj['text'](f"--- Shared with me ({len(items)}) ---\n")
        for i, item in enumerate(items):
            vis = item.get('resourceVisualization') or {}
            shared = item.get('lastShared') or {}
            by = shared.get('sharedBy') or {}
            dt = fmt_time(shared.get('sharedDateTime'))
            title = vis.get('title') or '(untitled)'
            item_type = vis.get('type') or ''
            who = by.get('displayName') or ''
            email = by.get('address') or ''
            via = shared.get('sharingType') or ''
            ctx.obj['text'](f"{str(i + 1).rjust(2)}. [{dt}] {title}")
            details = [item_type]
            if who:
                details.append(f"Shared by: {who}{f' <{email}>' if email else ''}")
            if via:
                details.append(via)
            ctx.obj['text'](f"    {'  |  '.join(details)}")
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))
