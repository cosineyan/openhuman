/**
 * Frontend API for topic threads — user-defined multi-dimensional topic
 * subscriptions that auto-aggregate matching memory chunks into a summary
 * timeline. Calls the openhuman.topic_threads_* RPC controllers.
 */
import { callCoreRpc } from '../coreRpcClient';

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type KeywordLogic = 'or' | 'and';

export interface TopicThread {
  id: string;
  name: string;
  description: string;
  keyword_logic: KeywordLogic;
  tree_id: string;
  created_at_ms: number;
  keywords: string[];
  source_pins: string[];
  entity_pins: string[];
  meeting_pins: string[];
}

export interface CreateTopicInput {
  name: string;
  description?: string;
  keyword_logic?: KeywordLogic;
  keywords?: string[];
  source_ids?: string[];
  entity_ids?: string[];
  meeting_names?: string[];
  backfill_days?: number;
}

export interface TopicTimelineNode {
  summary_id: string;
  level: number;
  time_range_start_ms: number;
  time_range_end_ms: number;
  body: string;
}

export interface TeamsConversation {
  conversation_id: string;
  source_id: string;
  label: string;
  chat_type: string | null;
  last_seen_ms: number | null;
  /** Store this as a source pin — matches the conversation's chunks. */
  pin_value: string;
}

export interface PersonEntity {
  entity_id: string;
  surface: string;
  kind: string;
  count: number;
}

export interface MeetingInfo {
  meeting_name: string;
  count: number;
  last_seen_ms: number | null;
}

export interface BackfillResult {
  scanned: number;
  matched: number;
  enqueued: number;
}

// ---------------------------------------------------------------------------
// API helpers
// ---------------------------------------------------------------------------

export async function listTopicThreads(): Promise<TopicThread[]> {
  const res = await callCoreRpc<{ result: TopicThread[]; logs: string[] }>({
    method: 'openhuman.topic_threads_list',
  });
  return res.result ?? [];
}

export async function createTopicThread(input: CreateTopicInput): Promise<TopicThread> {
  const res = await callCoreRpc<{ result: TopicThread; logs: string[] }>({
    method: 'openhuman.topic_threads_create',
    params: input as unknown as Record<string, unknown>,
  });
  return res.result;
}

export async function getTopicThread(id: string): Promise<TopicThread | null> {
  const res = await callCoreRpc<{ result: TopicThread | null; logs: string[] }>({
    method: 'openhuman.topic_threads_get',
    params: { id },
  });
  return res.result ?? null;
}

export async function updateTopicThread(
  id: string,
  patch: Partial<CreateTopicInput>
): Promise<TopicThread> {
  const res = await callCoreRpc<{ result: TopicThread; logs: string[] }>({
    method: 'openhuman.topic_threads_update',
    params: { id, ...patch } as unknown as Record<string, unknown>,
  });
  return res.result;
}

export async function deleteTopicThread(id: string): Promise<void> {
  await callCoreRpc<{ result: null; logs: string[] }>({
    method: 'openhuman.topic_threads_delete',
    params: { id },
  });
}

export async function topicThreadTimeline(id: string): Promise<TopicTimelineNode[]> {
  const res = await callCoreRpc<{ result: TopicTimelineNode[]; logs: string[] }>({
    method: 'openhuman.topic_threads_timeline',
    params: { id },
    timeoutMs: 45_000,
  });
  return res.result ?? [];
}

export async function discoverConversations(): Promise<TeamsConversation[]> {
  const res = await callCoreRpc<{ result: TeamsConversation[]; logs: string[] }>({
    method: 'openhuman.topic_threads_discover_conversations',
  });
  return res.result ?? [];
}

export async function discoverPeople(limit = 200): Promise<PersonEntity[]> {
  const res = await callCoreRpc<{ result: PersonEntity[]; logs: string[] }>({
    method: 'openhuman.topic_threads_discover_people',
    params: { limit },
    timeoutMs: 45_000,
  });
  return res.result ?? [];
}

/**
 * Resolve a pasted Teams chat deep link into a conversation pin with a real
 * label. Returns the TeamsConversation (its `pin_value` is stored as a source
 * pin). Throws with a user-facing message on parse / lookup failure.
 */
export async function resolveChatLink(url: string): Promise<TeamsConversation> {
  const res = await callCoreRpc<{ result: TeamsConversation; logs: string[] }>({
    method: 'openhuman.topic_threads_resolve_chat_link',
    params: { url },
    timeoutMs: 45_000,
  });
  return res.result;
}

export async function discoverMeetings(): Promise<MeetingInfo[]> {
  const res = await callCoreRpc<{ result: MeetingInfo[]; logs: string[] }>({
    method: 'openhuman.topic_threads_discover_meetings',
    timeoutMs: 45_000,
  });
  return res.result ?? [];
}

export async function backfillTopic(id: string, days: number): Promise<BackfillResult> {
  const res = await callCoreRpc<{ result: BackfillResult; logs: string[] }>({
    method: 'openhuman.topic_threads_backfill',
    params: { id, days },
    timeoutMs: 120_000,
  });
  return res.result;
}
