/**
 * Frontend client for the Kanban board surface (`openhuman.projects_*`).
 *
 * Wire shape note: the Rust handlers always use `RpcOutcome::single_log`,
 * which serialises as `{ result: <data>, logs: [...] }`. `callCoreRpc`
 * returns that envelope as-is (it only strips the outer JSON-RPC layer),
 * so every call here unwraps `.result` before returning to the caller.
 */
import debug from 'debug';

import { callCoreRpc } from '../coreRpcClient';

const log = debug('projectsApi');

// ---------------------------------------------------------------------------
// Domain types — mirrors of the Rust structs in src/openhuman/projects/types.rs
// ---------------------------------------------------------------------------

export interface Project {
  id: string;
  title: string;
  created: string;
  updated: string;
}

export interface Bucket {
  id: string;
  project_id: string;
  title: string;
  position: number;
  is_done_bucket: boolean;
  created: string;
  updated: string;
}

export interface Task {
  id: string;
  project_id: string;
  bucket_id: string;
  title: string;
  description: string | null;
  done: boolean;
  done_at: string | null;
  priority: number;
  due_date: string | null;
  hex_color: string | null;
  position: number;
  index: number;
  /** 'me' | 'ai' | null */
  assignee: string | null;
  /** Reserved for orchestrator use. Always null in Phase 1. */
  ai_plan: string | null;
  /** null = top-level task; set = subtask of that task id */
  parent_task_id: string | null;
  created: string;
  updated: string;
}

export interface BucketWithTasks {
  bucket: Bucket;
  tasks: Task[];
}

export interface BoardData {
  project: Project;
  buckets: BucketWithTasks[];
  /** task_id → [total_subtasks, done_subtasks] */
  subtask_counts: Record<string, [number, number]>;
}

export interface TaskEvent {
  id: string;
  task_id: string;
  kind: 'change' | 'comment';
  /** 'me' | 'ai' */
  actor: string;
  field?: string;
  old_value?: string;
  new_value?: string;
  body?: string;
  created: string;
}

export interface TaskAttachment {
  id: string;
  task_id: string;
  filename: string;
  mime_type: string;
  rel_path: string;
  size_bytes: number;
  /** 'me' | 'ai' */
  uploaded_by: string;
  created: string;
}

// ---------------------------------------------------------------------------
// Wire envelope — RpcOutcome::single_log always wraps in { result, logs }
// ---------------------------------------------------------------------------

interface RpcEnvelope<T> {
  result: T;
  logs: string[];
}

// ---------------------------------------------------------------------------
// API functions
// ---------------------------------------------------------------------------

/**
 * Fetch the full Kanban board: default project + all buckets with their tasks
 * grouped and ordered by position.
 */
export async function getBoard(): Promise<BoardData> {
  log('getBoard');
  const res = await callCoreRpc<RpcEnvelope<BoardData>>({
    method: 'openhuman.projects_get_board',
    params: {},
  });
  return res.result;
}

/** Create a new task in the default project. */
export async function createTask(params: {
  title: string;
  description?: string;
  bucket_id?: string;
  priority?: number;
  due_date?: string;
}): Promise<Task> {
  log('createTask title=%s', params.title);
  const res = await callCoreRpc<RpcEnvelope<Task>>({
    method: 'openhuman.projects_create_task',
    params,
  });
  return res.result;
}

/** Apply a partial patch to an existing task. */
export async function updateTask(params: {
  task_id: string;
  patch: {
    title?: string;
    description?: string | null;
    priority?: number;
    due_date?: string | null;
    hex_color?: string | null;
    position?: number;
    done?: boolean;
    /** 'me' | 'ai' | null to clear */
    assignee?: string | null;
  };
}): Promise<Task> {
  log('updateTask task_id=%s', params.task_id);
  const res = await callCoreRpc<RpcEnvelope<Task>>({
    method: 'openhuman.projects_update_task',
    params,
  });
  return res.result;
}

/** Move a task to a different bucket, optionally repositioning it. */
export async function moveTask(params: {
  task_id: string;
  bucket_id: string;
  position?: number;
}): Promise<Task> {
  log('moveTask task_id=%s bucket_id=%s', params.task_id, params.bucket_id);
  const res = await callCoreRpc<RpcEnvelope<Task>>({
    method: 'openhuman.projects_move_task',
    params,
  });
  return res.result;
}

/** Permanently delete a task by id. */
export async function deleteTask(task_id: string): Promise<void> {
  log('deleteTask task_id=%s', task_id);
  await callCoreRpc<RpcEnvelope<{ task_id: string; deleted: boolean }>>({
    method: 'openhuman.projects_delete_task',
    params: { task_id },
  });
}

/** Apply a partial patch to a bucket (rename, reorder, done-status). */
export async function updateBucket(params: {
  bucket_id: string;
  patch: { title?: string; position?: number; is_done_bucket?: boolean };
}): Promise<Bucket> {
  log('updateBucket bucket_id=%s', params.bucket_id);
  const res = await callCoreRpc<RpcEnvelope<Bucket>>({
    method: 'openhuman.projects_update_bucket',
    params,
  });
  return res.result;
}

/** Return all change-feed events and comments for a task, oldest first. */
export async function listTaskEvents(task_id: string): Promise<TaskEvent[]> {
  log('listTaskEvents task_id=%s', task_id);
  const res = await callCoreRpc<RpcEnvelope<TaskEvent[]>>({
    method: 'openhuman.projects_list_task_events',
    params: { task_id },
  });
  return res.result;
}

/** Add a plain-text comment to a task. */
export async function addComment(task_id: string, body: string, actor = 'me'): Promise<TaskEvent> {
  log('addComment task_id=%s actor=%s', task_id, actor);
  const res = await callCoreRpc<RpcEnvelope<TaskEvent>>({
    method: 'openhuman.projects_add_comment',
    params: { task_id, body, actor },
  });
  return res.result;
}

/** Attach a file by its absolute path. uploaded_by defaults to 'me'. */
export async function addAttachment(params: {
  task_id: string;
  src_path: string;
  uploaded_by?: string;
}): Promise<TaskAttachment> {
  log('addAttachment task_id=%s', params.task_id);
  const res = await callCoreRpc<RpcEnvelope<TaskAttachment>>({
    method: 'openhuman.projects_add_attachment',
    params: { uploaded_by: 'me', ...params },
  });
  return res.result;
}

/** List all attachments for a task. */
export async function listAttachments(task_id: string): Promise<TaskAttachment[]> {
  log('listAttachments task_id=%s', task_id);
  const res = await callCoreRpc<RpcEnvelope<TaskAttachment[]>>({
    method: 'openhuman.projects_list_attachments',
    params: { task_id },
  });
  return res.result;
}

/** Delete an attachment by id. */
export async function deleteAttachment(attachment_id: string): Promise<void> {
  log('deleteAttachment attachment_id=%s', attachment_id);
  await callCoreRpc<RpcEnvelope<unknown>>({
    method: 'openhuman.projects_delete_attachment',
    params: { attachment_id },
  });
}

/** Delete a subtask by id (logs deletion on the parent's feed). */
export async function deleteSubtask(task_id: string): Promise<void> {
  log('deleteSubtask task_id=%s', task_id);
  await callCoreRpc<RpcEnvelope<unknown>>({
    method: 'openhuman.projects_delete_subtask',
    params: { task_id },
  });
}

/** Return all subtasks for a given parent task id. */
export async function listSubtasks(parent_task_id: string): Promise<Task[]> {
  log('listSubtasks parent_task_id=%s', parent_task_id);
  const res = await callCoreRpc<RpcEnvelope<Task[]>>({
    method: 'openhuman.projects_list_subtasks',
    params: { parent_task_id },
  });
  return res.result;
}

/** Create a subtask under a parent task. */
export async function createSubtask(params: {
  parent_task_id: string;
  title: string;
}): Promise<Task> {
  log('createSubtask parent=%s title=%s', params.parent_task_id, params.title);
  const res = await callCoreRpc<RpcEnvelope<Task>>({
    method: 'openhuman.projects_create_subtask',
    params,
  });
  return res.result;
}

/** Hard-cancel a running AI task. Returns true if it was found and stopped. */
export async function cancelAiTask(task_id: string): Promise<{ cancelled: boolean }> {
  log('cancelAiTask task_id=%s', task_id);
  const res = await callCoreRpc<RpcEnvelope<{ cancelled: boolean }>>({
    method: 'openhuman.projects_cancel_ai_task',
    params: { task_id },
  });
  return res.result;
}

/** Return the IDs of all tasks currently being processed by the AI runner. */
export async function listRunningAiTasks(): Promise<{ task_ids: string[] }> {
  log('listRunningAiTasks');
  const res = await callCoreRpc<RpcEnvelope<{ task_ids: string[] }>>({
    method: 'openhuman.projects_list_running_ai_tasks',
    params: {},
  });
  return res.result;
}
