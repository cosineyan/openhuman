/**
 * Frontend API for the bundled m365-cli — token status and auth management.
 * Calls the openhuman.m365_* RPC controllers via the core RPC relay.
 */
import { callCoreRpc } from '../coreRpcClient';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export interface M365TokenEntry {
  valid: boolean;
  cached: boolean;
  expiresInMin: number | null;
  sessionId: string | null;
}

export interface M365TokenStatus {
  ok: boolean;
  graph: M365TokenEntry;
  rest: M365TokenEntry;
  teams: M365TokenEntry;
  // SAP additional services
  aha?: M365TokenEntry;
  jira?: M365TokenEntry;
  wiki?: M365TokenEntry;
  sharepoint?: M365TokenEntry;
}

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

export interface MpcChromeStatus {
  ok: boolean;
  port: number;
  error?: string;
}

export async function getMcpChromeStatus(): Promise<MpcChromeStatus> {
  return callCoreRpc<MpcChromeStatus>({ method: 'openhuman.m365_mcp_chrome_status', params: {} });
}

export async function getM365TokenStatus(): Promise<M365TokenStatus> {
  return callCoreRpc<M365TokenStatus>({ method: 'openhuman.m365_token_status', params: {} });
}

export async function m365AuthLogin(): Promise<M365TokenStatus> {
  return callCoreRpc<M365TokenStatus>({
    method: 'openhuman.m365_auth_login',
    params: {},
    timeoutMs: 120_000,
  });
}

export async function m365AuthRefresh(): Promise<M365TokenStatus> {
  return callCoreRpc<M365TokenStatus>({
    method: 'openhuman.m365_auth_refresh',
    params: {},
    timeoutMs: 120_000,
  });
}

export async function m365AuthLogout(): Promise<void> {
  await callCoreRpc({ method: 'openhuman.m365_auth_logout', params: {} });
}

export async function m365SetAhaToken(token: string): Promise<void> {
  await callCoreRpc({ method: 'openhuman.m365_set_aha_token', params: { token } });
}

export async function m365ClearAhaToken(): Promise<void> {
  await callCoreRpc({ method: 'openhuman.m365_clear_aha_token', params: {} });
}

export async function m365RefreshSharePoint(): Promise<void> {
  await callCoreRpc({ method: 'openhuman.m365_refresh_sharepoint', params: {}, timeoutMs: 30_000 });
}

export async function m365OpenInChrome(url: string): Promise<void> {
  await callCoreRpc({ method: 'openhuman.m365_open_in_chrome', params: { url } });
}
