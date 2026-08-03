/**
 * Claude Code settings-profile registry API.
 *
 * Thin wrappers over the `openhuman.claude_profiles_*` RPCs. A profile
 * registers a path to one of the user's Claude Code `settings.json.*` files;
 * the core parses each file's model tiers (opus/sonnet/haiku/default) and
 * NEVER returns any auth token.
 */
import { isTauri } from '../../utils/tauriCommands/common';
import { callCoreRpc } from '../coreRpcClient';

/** Parsed model tiers for a profile. Never contains secrets. */
export interface ProfileModels {
  opus?: string;
  sonnet?: string;
  haiku?: string;
  default?: string;
}

export interface ClaudeProfile {
  id: string;
  name: string;
  path: string;
}

export interface ProfileWithModels {
  profile: ClaudeProfile;
  models: ProfileModels;
  /** False when the settings.json at `profile.path` is missing/unreadable. */
  readable: boolean;
}

/** List all registered profiles with their parsed models. */
export async function listProfiles(): Promise<ProfileWithModels[]> {
  if (!isTauri()) return [];
  const res = await callCoreRpc<{ profiles: ProfileWithModels[] }>({
    method: 'openhuman.claude_profiles_list_profiles',
  });
  return res.profiles ?? [];
}

/** Register a settings.json path as a profile. */
export async function addProfile(name: string, path: string): Promise<ProfileWithModels> {
  const res = await callCoreRpc<{ profile: ProfileWithModels }>({
    method: 'openhuman.claude_profiles_add_profile',
    params: { name, path },
  });
  return res.profile;
}

/** Remove a profile by id. */
export async function removeProfile(id: string): Promise<boolean> {
  const res = await callCoreRpc<{ removed: boolean }>({
    method: 'openhuman.claude_profiles_remove_profile',
    params: { id },
  });
  return res.removed;
}

/** Get one profile (with parsed models) by id. */
export async function getProfile(id: string): Promise<ProfileWithModels> {
  const res = await callCoreRpc<{ profile: ProfileWithModels }>({
    method: 'openhuman.claude_profiles_get_profile',
    params: { id },
  });
  return res.profile;
}

/** Parse the models at a path WITHOUT registering it — live preview. */
export async function previewModels(
  path: string
): Promise<{ models: ProfileModels; readable: boolean }> {
  return await callCoreRpc<{ models: ProfileModels; readable: boolean }>({
    method: 'openhuman.claude_profiles_preview_models',
    params: { path },
  });
}

/** One resolved step on the global fallback ladder. */
export interface LadderStepResolved {
  profile_id: string;
  profile_name: string;
  tier: string;
  model?: string;
  readable: boolean;
}

/** A ladder step as stored (order = fallback order). */
export interface LadderStep {
  profile_id: string;
  tier: string;
}

/** Get the global fallback ladder (resolved). Empty stored → auto-prefill. */
export async function getLadder(): Promise<LadderStepResolved[]> {
  if (!isTauri()) return [];
  const res = await callCoreRpc<{ ladder: LadderStepResolved[] }>({
    method: 'openhuman.claude_profiles_get_ladder',
  });
  return res.ladder ?? [];
}

/** Persist a new ladder order. */
export async function setLadder(steps: LadderStep[]): Promise<void> {
  await callCoreRpc<{ ok: boolean }>({
    method: 'openhuman.claude_profiles_set_ladder',
    params: { steps },
  });
}

/** Encode a ladder step as the task `fallback_end` string. */
export const encodeStep = (profileId: string, tier: string): string => `${profileId}:${tier}`;

/** Global default fallback policy (for tasks without their own profile). */
export interface GlobalFallback {
  enabled: boolean;
  start_profile?: string;
  start_tier?: string;
  direction?: string;
  end?: string;
}

/** Get the global default fallback policy. */
export async function getGlobalFallback(): Promise<GlobalFallback> {
  if (!isTauri()) return { enabled: false };
  const res = await callCoreRpc<{ global_fallback: GlobalFallback }>({
    method: 'openhuman.claude_profiles_get_global_fallback',
  });
  return res.global_fallback ?? { enabled: false };
}

/** Persist the global default fallback policy. */
export async function setGlobalFallback(gf: GlobalFallback): Promise<void> {
  await callCoreRpc<{ ok: boolean }>({
    method: 'openhuman.claude_profiles_set_global_fallback',
    params: { global_fallback: gf },
  });
}

/** The tier keys a picker can offer, in display order. */
export const PROFILE_TIERS: Array<keyof ProfileModels> = ['default', 'opus', 'sonnet', 'haiku'];

/** Tier aliases present (non-empty) on a profile, for building a model dropdown. */
export function availableTiers(models: ProfileModels): Array<keyof ProfileModels> {
  return PROFILE_TIERS.filter(t => !!models[t]);
}
