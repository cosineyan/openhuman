import urllib.parse
import click
from ..api import graph_request

USER_ALL_SELECT = 'aboutMe,accountEnabled,ageGroup,assignedLicenses,assignedPlans,birthday,businessPhones,city,companyName,consentProvidedForMinor,country,createdDateTime,creationType,department,displayName,employeeHireDate,employeeId,employeeOrgData,employeeType,externalUserState,faxNumber,givenName,hireDate,id,identities,imAddresses,interests,isResourceAccount,jobTitle,lastPasswordChangeDateTime,legalAgeGroupClassification,mail,mailNickname,mobilePhone,mySite,officeLocation,onPremisesDistinguishedName,onPremisesDomainName,onPremisesExtensionAttributes,onPremisesImmutableId,onPremisesLastSyncDateTime,onPremisesProvisioningErrors,onPremisesSamAccountName,onPremisesSecurityIdentifier,onPremisesSyncEnabled,onPremisesUserPrincipalName,otherMails,passwordPolicies,pastProjects,postalCode,preferredLanguage,preferredName,provisionedPlans,proxyAddresses,responsibilities,schools,showInAddressList,signInSessionsValidFromDateTime,skills,state,streetAddress,surname,usageLocation,userPrincipalName,userType'


def format_person(p, text_fn):
    emails = p.get('scoredEmailAddresses') or []
    email = (emails[0].get('address') if emails else None) or p.get('userPrincipalName') or ''
    parts = [p.get('displayName') or '(unknown)']
    if email:
        parts.append(f'<{email}>')
    if p.get('jobTitle'):
        parts.append(f'— {p["jobTitle"]}')
    if p.get('department'):
        parts.append(f'| {p["department"]}')
    text_fn(f'  {" ".join(parts)}')

    details = []
    if p.get('companyName'):
        details.append(f'company: {p["companyName"]}')
    if p.get('officeLocation'):
        details.append(f'office: {p["officeLocation"]}')
    phones = [f'{ph["type"]}: {ph["number"]}' for ph in (p.get('phones') or []) if ph.get('number')]
    if phones:
        details.append(', '.join(phones))
    if details:
        text_fn(f'    {" | ".join(details)}')


def format_user(u, text_fn):
    parts = [u.get('displayName') or '(unknown)']
    if u.get('mailNickname'):
        parts.append(f'({u["mailNickname"]})')
    if u.get('mail'):
        parts.append(f'<{u["mail"]}>')
    if u.get('jobTitle'):
        parts.append(f'— {u["jobTitle"]}')
    if u.get('department'):
        parts.append(f'| {u["department"]}')
    text_fn(f'  {" ".join(parts)}')

    details = []
    if u.get('companyName'):
        details.append(f'company: {u["companyName"]}')
    if u.get('officeLocation'):
        details.append(f'office: {u["officeLocation"]}')
    if u.get('businessPhones'):
        details.append(f'phone: {", ".join(u["businessPhones"])}')
    if u.get('mobilePhone'):
        details.append(f'mobile: {u["mobilePhone"]}')
    if details:
        text_fn(f'    {" | ".join(details)}')


@click.group('people')
def people_cmd():
    """People search and lookup (Graph People API)."""


@people_cmd.command('search')
@click.argument('query')
@click.option('-n', '--top', default='10')
@click.option('--org', is_flag=True, help='Only organization users')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def people_search(ctx, query, top, org, as_json):
    """Search people by name, email, or topic."""
    try:
        endpoint = f'people?$search="{urllib.parse.quote(query)}"&$top={int(top)}'
        if org:
            endpoint += "&$filter=personType/class eq 'Person' and personType/subclass eq 'OrganizationUser'"
        result = graph_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        persons = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': persons})
        if not persons:
            return ctx.obj['text']('No people found.')
        ctx.obj['text'](f'--- People ({len(persons)} results) ---\n')
        for p in persons:
            format_person(p, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@people_cmd.command('list')
@click.option('-n', '--top', default='10')
@click.option('--org', is_flag=True, help='Only organization users')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def people_list(ctx, top, org, as_json):
    """List people most relevant to you."""
    try:
        endpoint = f'people?$top={int(top)}'
        if org:
            endpoint += "&$filter=personType/class eq 'Person' and personType/subclass eq 'OrganizationUser'"
        result = graph_request(endpoint)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        persons = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': persons})
        if not persons:
            return ctx.obj['text']('No people found.')
        ctx.obj['text'](f'--- Relevant people ({len(persons)}) ---\n')
        for p in persons:
            format_person(p, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@people_cmd.command('manager')
@click.argument('identifier', required=False)
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def people_manager(ctx, identifier, as_json):
    """Get manager of a person (default: yourself)."""
    try:
        base = f'users/{urllib.parse.quote(identifier)}' if identifier else 'me'
        select = USER_ALL_SELECT if as_json else 'displayName,mailNickname,mail,jobTitle,department,officeLocation,companyName,businessPhones,mobilePhone'
        result = graph_request(f'{base}/manager?$select={select}', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        u = result['body']
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': u})
        format_user(u, ctx.obj['text'])
    except Exception as e:
        ctx.obj['die'](str(e))


@people_cmd.command('reports')
@click.argument('identifier', required=False)
@click.option('-n', '--top', default='50')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def people_reports(ctx, identifier, top, as_json):
    """Get direct reports of a person (default: yourself)."""
    try:
        base = f'users/{urllib.parse.quote(identifier)}' if identifier else 'me'
        select = USER_ALL_SELECT if as_json else 'displayName,mailNickname,mail,jobTitle,department,officeLocation,companyName,businessPhones,mobilePhone'
        result = graph_request(f'{base}/directReports?$select={select}&$top={int(top)}', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        reports = (result['body'] or {}).get('value') or []
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': reports})
        if not reports:
            return ctx.obj['text']('No direct reports found.')
        ctx.obj['text'](f'--- Direct Reports ({len(reports)}) ---\n')
        for u in reports:
            format_user(u, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@people_cmd.command('peers')
@click.argument('identifier', required=False)
@click.option('-n', '--top', default='50')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def people_peers(ctx, identifier, top, as_json):
    """Get peers (manager's other direct reports, excluding yourself)."""
    try:
        base = f'users/{urllib.parse.quote(identifier)}' if identifier else 'me'
        me_result = graph_request(f'{base}?$select=mail', raw=True)
        if me_result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {me_result["status_code"]} {me_result["body"]}')
        my_mail = (me_result['body'].get('mail') or '').lower()

        mgr_result = graph_request(f'{base}/manager?$select=id', raw=True)
        if mgr_result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {mgr_result["status_code"]} {mgr_result["body"]}')
        mgr_id = mgr_result['body']['id']

        select = USER_ALL_SELECT if as_json else 'displayName,mailNickname,mail,jobTitle,department,officeLocation,companyName,businessPhones,mobilePhone'
        rpt_result = graph_request(f'users/{mgr_id}/directReports?$select={select}&$top={int(top)}', raw=True)
        if rpt_result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {rpt_result["status_code"]} {rpt_result["body"]}')

        peers = [u for u in ((rpt_result['body'] or {}).get('value') or []) if (u.get('mail') or '').lower() != my_mail]
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': peers})
        if not peers:
            return ctx.obj['text']('No peers found.')
        ctx.obj['text'](f'--- Peers ({len(peers)}) ---\n')
        for u in peers:
            format_user(u, ctx.obj['text'])
            ctx.obj['text']('')
    except Exception as e:
        ctx.obj['die'](str(e))


@people_cmd.command('get')
@click.argument('identifier')
@click.option('--json', 'as_json', is_flag=True)
@click.pass_context
def people_get(ctx, identifier, as_json):
    """Get details for a specific person by email or user ID."""
    try:
        select = f'?$select={USER_ALL_SELECT}' if as_json else ''
        result = graph_request(f'users/{urllib.parse.quote(identifier)}{select}', raw=True)
        if result['status_code'] != 200:
            return ctx.obj['die'](f'Graph API error: {result["status_code"]} {result["body"]}')
        u = result['body']
        if as_json:
            return ctx.obj['out']({'ok': True, 'data': u})
        ctx.obj['text'](f'{u.get("displayName") or "(unknown)"} ({u.get("mailNickname") or ""}) <{u.get("mail") or u.get("userPrincipalName") or ""}>')
        if u.get('jobTitle'):
            ctx.obj['text'](f'Title: {u["jobTitle"]}')
        if u.get('department'):
            ctx.obj['text'](f'Department: {u["department"]}')
        if u.get('companyName'):
            ctx.obj['text'](f'Company: {u["companyName"]}')
        if u.get('officeLocation'):
            ctx.obj['text'](f'Office: {u["officeLocation"]}')
        if u.get('businessPhones'):
            ctx.obj['text'](f'Phone: {", ".join(u["businessPhones"])}')
        if u.get('mobilePhone'):
            ctx.obj['text'](f'Mobile: {u["mobilePhone"]}')
        if u.get('id'):
            ctx.obj['text'](f'ID: {u["id"]}')
    except Exception as e:
        ctx.obj['die'](str(e))
