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
}

interface RpcEnvelope<T> {
  result: T;
  logs: string[];
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
  const res = await callCoreRpc<RpcEnvelope<MpcChromeStatus>>({
    method: 'openhuman.m365_mcp_chrome_status',
    params: {},
  });
  return res.result;
}

export async function getM365TokenStatus(): Promise<M365TokenStatus> {
  const res = await callCoreRpc<RpcEnvelope<M365TokenStatus>>({
    method: 'openhuman.m365_token_status',
    params: {},
  });
  return res.result;
}

export async function m365AuthLogin(): Promise<M365TokenStatus> {
  const res = await callCoreRpc<RpcEnvelope<M365TokenStatus>>({
    method: 'openhuman.m365_auth_login',
    params: {},
  });
  return res.result;
}

export async function m365AuthRefresh(): Promise<M365TokenStatus> {
  const res = await callCoreRpc<RpcEnvelope<M365TokenStatus>>({
    method: 'openhuman.m365_auth_refresh',
    params: {},
  });
  return res.result;
}

export async function m365AuthLogout(): Promise<void> {
  await callCoreRpc({ method: 'openhuman.m365_auth_logout', params: {} });
}
