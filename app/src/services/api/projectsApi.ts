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
  patch: {
    title?: string;
    position?: number;
    is_done_bucket?: boolean;
  };
}): Promise<Bucket> {
  log('updateBucket bucket_id=%s', params.bucket_id);
  const res = await callCoreRpc<RpcEnvelope<Bucket>>({
    method: 'openhuman.projects_update_bucket',
    params,
  });
  return res.result;
}
