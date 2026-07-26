import { callCoreRpc } from '../coreRpcClient';

// ---------------------------------------------------------------------------
// Types — mirrors of Rust structs in email_automation/types.rs
// ---------------------------------------------------------------------------

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
  await callCoreRpc({
    method: 'openhuman.email_automation_delete_rule',
    params: { id },
  });
}

export async function runNow(lastN = 50): Promise<RunNowResult> {
  const res = await callCoreRpc<RpcEnvelope<RunNowResult>>({
    method: 'openhuman.email_automation_run_now',
    params: { last_n: lastN },
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
