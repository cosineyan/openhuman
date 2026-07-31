import { callCoreRpc } from '../coreRpcClient';

// ---------------------------------------------------------------------------
// Types — mirrors of Rust structs in email_automation/types.rs
// ---------------------------------------------------------------------------

export type BatchParseMode = 'first_only' | 'all';

export interface EmailAutomationRule {
  id: string;
  name: string;
  enabled: boolean;
  sender_contains: string | null;
  subject_contains: string | null;
  body_contains: string | null;
  task_title_template: string;
  task_description_template: string | null;
  assignee: string;
  bucket_id: string | null;
  llm_fallback_enabled: boolean;
  parse_script: string | null;
  batch_mode: boolean;
  batch_window_secs: number;
  batch_parse_mode: BatchParseMode;
  created_at: string;
  updated_at: string;
}

export interface CreateRuleInput {
  name: string;
  enabled?: boolean;
  sender_contains?: string | null;
  subject_contains?: string | null;
  body_contains?: string | null;
  task_title_template: string;
  task_description_template?: string | null;
  assignee?: string;
  bucket_id?: string | null;
  llm_fallback_enabled?: boolean;
  parse_script?: string | null;
  batch_mode?: boolean;
  batch_window_secs?: number;
  batch_parse_mode?: BatchParseMode;
}

export interface RulePatch {
  name?: string;
  enabled?: boolean;
  sender_contains?: string | null;
  subject_contains?: string | null;
  body_contains?: string | null;
  task_title_template?: string;
  task_description_template?: string | null;
  assignee?: string;
  bucket_id?: string | null;
  llm_fallback_enabled?: boolean;
  parse_script?: string | null;
  batch_mode?: boolean;
  batch_window_secs?: number;
  batch_parse_mode?: BatchParseMode;
}

export interface RuleHit {
  rule_id: string;
  rule_name: string;
  task_title: string;
}

export interface RunNowResult {
  emails_scanned: number;
  tasks_created: number;
  hits: RuleHit[];
}

export interface EmailChunkSummary {
  chunk_id: string;
  subject: string;
  sender: string;
  date: string;
  preview: string;
}

interface RpcEnvelope<T> {
  result: T;
  logs: string[];
}

// ---------------------------------------------------------------------------
// API calls
// ---------------------------------------------------------------------------

export async function listRules(): Promise<EmailAutomationRule[]> {
  const res = await callCoreRpc<RpcEnvelope<EmailAutomationRule[]>>({
    method: 'openhuman.email_automation_list_rules',
    params: {},
  });
  return res.result;
}

export async function createRule(input: CreateRuleInput): Promise<EmailAutomationRule> {
  const res = await callCoreRpc<RpcEnvelope<EmailAutomationRule>>({
    method: 'openhuman.email_automation_create_rule',
    params: input,
  });
  return res.result;
}

export async function updateRule(id: string, patch: RulePatch): Promise<EmailAutomationRule> {
  const res = await callCoreRpc<RpcEnvelope<EmailAutomationRule>>({
    method: 'openhuman.email_automation_update_rule',
    params: { id, ...patch },
  });
  return res.result;
}

export async function deleteRule(id: string): Promise<void> {
  await callCoreRpc({ method: 'openhuman.email_automation_delete_rule', params: { id } });
}

export async function runNow(lastN = 50, hours?: number): Promise<RunNowResult> {
  const res = await callCoreRpc<RpcEnvelope<RunNowResult>>({
    method: 'openhuman.email_automation_run_now',
    params: { last_n: lastN, ...(hours !== undefined ? { hours } : {}) },
    timeoutMs: 120_000, // 2 minutes — Graph API + parse_script can be slow
  });
  return res.result;
}

export async function searchEmailChunks(params?: {
  sender_filter?: string;
  subject_filter?: string;
  limit?: number;
}): Promise<EmailChunkSummary[]> {
  const res = await callCoreRpc<RpcEnvelope<EmailChunkSummary[]>>({
    method: 'openhuman.email_automation_search_email_chunks',
    params: { limit: 10, ...params },
  });
  return res.result;
}

export async function generateRuleFromEmail(chunkId: string): Promise<CreateRuleInput> {
  const res = await callCoreRpc<RpcEnvelope<CreateRuleInput>>({
    method: 'openhuman.email_automation_generate_rule_from_email',
    params: { chunk_id: chunkId },
  });
  return res.result;
}

export async function generateRuleFromEmails(chunkIds: string[]): Promise<CreateRuleInput> {
  const res = await callCoreRpc<RpcEnvelope<CreateRuleInput>>({
    method: 'openhuman.email_automation_generate_rule_from_emails',
    params: { chunk_ids: chunkIds },
  });
  return res.result;
}

export interface DryRunResult {
  title: string;
  description: string | null;
  parsed_vars: Record<string, unknown>;
  script_error: string | null;
}

export interface ProcessedEmailEntry {
  source_id: string;
  rule_id: string;
  rule_name: string;
  task_id: string;
  processed_at: string;
}

export interface EmailContentResult {
  subject: string;
  from: string;
  to: string;
  date: string;
  body: string;
}

export async function getEmailContent(sourceId: string): Promise<EmailContentResult | null> {
  const res = await callCoreRpc<RpcEnvelope<EmailContentResult | null>>({
    method: 'openhuman.email_automation_get_email_content',
    params: { source_id: sourceId },
  });
  return res.result;
}

export async function listProcessedEmails(limit = 100): Promise<ProcessedEmailEntry[]> {
  const res = await callCoreRpc<RpcEnvelope<ProcessedEmailEntry[]>>({
    method: 'openhuman.email_automation_list_processed_emails',
    params: { limit },
  });
  return res.result;
}

export async function dryRunRule(params: {
  task_title_template: string;
  task_description_template?: string | null;
  parse_script?: string | null;
  email_body?: string;
  chunk_id?: string;
}): Promise<DryRunResult> {
  const res = await callCoreRpc<RpcEnvelope<DryRunResult>>({
    method: 'openhuman.email_automation_dry_run',
    params,
  });
  return res.result;
}

export async function refineRule(params: {
  task_title_template: string;
  task_description_template?: string | null;
  parse_script?: string | null;
  email_body?: string;
  chunk_id?: string;
  user_feedback: string;
}): Promise<CreateRuleInput> {
  const res = await callCoreRpc<RpcEnvelope<CreateRuleInput>>({
    method: 'openhuman.email_automation_refine_rule',
    params,
    timeoutMs: 120_000,
  });
  return res.result;
}
