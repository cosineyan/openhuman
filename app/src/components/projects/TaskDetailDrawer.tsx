import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

import { formatFileSize } from '../../lib/attachments';
import { BubbleMarkdown } from '../../pages/conversations/components/AgentMessageBubble';
import {
  addAttachment,
  addComment,
  type Bucket,
  createSubtask,
  deleteAttachment,
  deleteSubtask,
  listAttachments,
  listSubtasks,
  listTaskEvents,
  type Task,
  type TaskAttachment,
  type TaskEvent,
} from '../../services/api/projectsApi';
import { openWorkspacePath, revealWorkspacePath } from '../../utils/tauriCommands/workspacePaths';
import { AiRunDrawer } from './AiRunDrawer';
import { ClaudeCodeResumeCard } from './ClaudeCodeResumeCard';
import { useAiTaskRuns } from './useAiTaskRuns';

/** Extract claude_session_id from task.ai_plan JSON, or null if absent/invalid. */
interface ClaudeResumeInfo {
  sessionId: string;
  workspaceDir: string | null;
}

function parseClaudeSessionId(aiPlan: string | null | undefined): ClaudeResumeInfo | null {
  if (!aiPlan) return null;
  try {
    const parsed = JSON.parse(aiPlan) as unknown;
    if (
      parsed !== null &&
      typeof parsed === 'object' &&
      'claude_session_id' in parsed &&
      typeof (parsed as Record<string, unknown>).claude_session_id === 'string' &&
      (parsed as Record<string, unknown>).claude_session_id !== ''
    ) {
      const sessionId = (parsed as Record<string, unknown>).claude_session_id as string;
      const workspaceDir =
        typeof (parsed as Record<string, unknown>).claude_workspace_dir === 'string'
          ? ((parsed as Record<string, unknown>).claude_workspace_dir as string)
          : null;
      return { sessionId, workspaceDir };
    }
  } catch {
    // malformed JSON — silent
  }
  return null;
}

interface SavePatch {
  title?: string;
  description?: string | null;
  priority?: number;
  due_date?: string | null;
  assignee?: string | null;
}

interface Props {
  task: Task | null;
  buckets: Bucket[];
  parentTask?: Task | null;
  onClose: () => void;
  onBack?: () => void;
  onSave: (taskId: string, patch: SavePatch) => Promise<void>;
  onDelete: (taskId: string) => Promise<void>;
  onMove: (taskId: string, bucketId: string) => Promise<void>;
  onSubtaskClick: (subtask: Task, parent: Task) => void;
  /** When set, modal opens in create-task mode for this bucket id. */
  createBucketId?: string | null;
  onCreateTask?: (
    bucketId: string,
    title: string,
    patch: Omit<SavePatch, 'title'>
  ) => Promise<void>;
}

const PRIORITIES = [
  { value: 0, label: 'None' },
  { value: 1, label: 'Low' },
  { value: 2, label: 'Medium' },
  { value: 3, label: 'High' },
  { value: 4, label: 'Urgent' },
  { value: 5, label: 'Critical' },
];

const ASSIGNEES = [
  { value: '', label: '— Unassigned' },
  { value: 'me', label: 'Me' },
  { value: 'ai', label: 'AI' },
];

const FIELD_LABELS: Record<string, string> = {
  bucket_id: 'Status',
  priority: 'Priority',
  done: 'Done',
  title: 'Title',
  description: 'Description',
  assignee: 'Assignee',
  due_date: 'Due date',
  created: 'Task',
  attachment: 'Attachment',
  attachment_removed: 'Attachment',
  subtask_added: 'Subtask',
  subtask_removed: 'Subtask',
  subtask_updated: 'Subtask',
};

const PRIORITY_NAMES: Record<string, string> = {
  '0': 'None',
  '1': 'Low',
  '2': 'Medium',
  '3': 'High',
  '4': 'Urgent',
  '5': 'Critical',
};

type FeedFilter = 'all' | 'comments' | 'attachments';

function humanizeValue(field: string, value: string | undefined, buckets: Bucket[]): string {
  if (value === undefined || value === null) return '—';
  if (field === 'bucket_id') return buckets.find(b => b.id === value)?.title ?? value;
  if (field === 'priority') return PRIORITY_NAMES[value] ?? value;
  if (field === 'done') return value === 'true' ? 'Done' : 'Not done';
  return value;
}

function formatTime(iso: string): string {
  return new Date(iso).toLocaleString(undefined, {
    month: 'short',
    day: 'numeric',
    hour: '2-digit',
    minute: '2-digit',
  });
}

export function TaskDetailDrawer({
  task,
  buckets,
  parentTask,
  onClose,
  onBack,
  onSave,
  onDelete,
  onMove,
  onSubtaskClick,
  createBucketId,
  onCreateTask,
}: Props) {
  const isCreateMode = !task && !!createBucketId;

  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState(0);
  const [dueDate, setDueDate] = useState('');
  const [assignee, setAssignee] = useState('');
  const [bucketId, setBucketId] = useState(createBucketId ?? '');
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  const [subtasks, setSubtasks] = useState<Task[]>([]);
  const [newSubtaskTitle, setNewSubtaskTitle] = useState('');
  const [addingSubtask, setAddingSubtask] = useState(false);

  const [events, setEvents] = useState<TaskEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [feedFilter, setFeedFilter] = useState<FeedFilter>('all');
  const [commentDraft, setCommentDraft] = useState('');
  const [expandedComment, setExpandedComment] = useState<string | null>(null);
  const [submittingComment, setSubmittingComment] = useState(false);
  const feedEndRef = useRef<HTMLDivElement>(null);

  const [attachments, setAttachments] = useState<TaskAttachment[]>([]);
  const [attachUploading, setAttachUploading] = useState(false);
  const [showRunDrawer, setShowRunDrawer] = useState(false);

  const { getRun } = useAiTaskRuns();
  const activeRun = task ? getRun(task.id) : undefined;
  const prevRunStatusRef = useRef<string | undefined>(undefined);

  const loadEvents = useCallback(async (taskId: string) => {
    setEventsLoading(true);
    try {
      const data = await listTaskEvents(taskId);
      setEvents(data);
    } finally {
      setEventsLoading(false);
    }
  }, []);

  // When the AI run transitions from running → terminal, reload events so the
  // new comments and bucket-change entries appear without needing a page switch.
  useEffect(() => {
    const prev = prevRunStatusRef.current;
    const curr = activeRun?.status;
    prevRunStatusRef.current = curr;
    if (prev === 'running' && curr !== 'running' && task) {
      void loadEvents(task.id);
    }
  }, [activeRun?.status, task, loadEvents]);

  const loadAttachments = useCallback(async (taskId: string) => {
    try {
      const data = await listAttachments(taskId);
      setAttachments(data);
    } catch {
      // non-fatal
    }
  }, []);

  const loadSubtasks = useCallback(async (taskId: string) => {
    try {
      const data = await listSubtasks(taskId);
      setSubtasks(data);
    } catch {
      // non-fatal
    }
  }, []);

  const prevTaskIdRef = useRef<string | null>(null);
  const prevEventCountRef = useRef(0);

  useEffect(() => {
    if (task) {
      // Only reset form fields when the task ID changes (different task opened).
      // Skipping resets on poll-refresh avoids clearing user's in-progress edits.
      if (task.id !== prevTaskIdRef.current) {
        prevTaskIdRef.current = task.id;
        prevEventCountRef.current = 0;
        setTitle(task.title);
        setDescription(task.description ?? '');
        setPriority(task.priority);
        setDueDate(task.due_date ? task.due_date.slice(0, 10) : '');
        setAssignee(task.assignee ?? '');
        setBucketId(task.bucket_id);
        setConfirmDelete(false);
        setCommentDraft('');
        setNewSubtaskTitle('');
        void loadEvents(task.id);
        void loadAttachments(task.id);
        void loadSubtasks(task.id);
      } else {
        // On poll-refresh of the same task, update non-editable fields only.
        setPriority(task.priority);
        setAssignee(task.assignee ?? '');
        setBucketId(task.bucket_id);
      }
    } else if (createBucketId) {
      setTitle('');
      setDescription('');
      setPriority(0);
      setDueDate('');
      setAssignee('');
      setBucketId(createBucketId);
      setConfirmDelete(false);
      setSubtasks([]);
      prevTaskIdRef.current = null;
    }
  }, [task, createBucketId, loadEvents, loadAttachments, loadSubtasks]);

  // When the board poller refreshes the task object (e.g. AI moved it to Done),
  // reload events so the change feed stays current without requiring a page switch.
  // Gated on `task.updated` so we don't fire on every poller tick when nothing changed.
  const prevTaskUpdatedRef = useRef<string | undefined>(undefined);
  useEffect(() => {
    if (!task) return;
    if (prevTaskUpdatedRef.current !== undefined && prevTaskUpdatedRef.current !== task.updated) {
      void loadEvents(task.id);
    }
    prevTaskUpdatedRef.current = task.updated;
  }, [task?.updated, task?.id, loadEvents]);

  useEffect(() => {
    if (feedFilter === 'attachments') return;
    // Only scroll to bottom when new events appear, not on every reload.
    if (events.length > prevEventCountRef.current) {
      feedEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
    prevEventCountRef.current = events.length;
  }, [events, feedFilter]);

  const currentBucket = buckets.find(b => b.id === (task?.bucket_id ?? bucketId));
  const isTerminalState =
    currentBucket?.is_done_bucket === true ||
    (currentBucket?.title.toLowerCase().includes('block') ?? false);
  const claudeResumeInfo = task ? parseClaudeSessionId(task.ai_plan) : null;
  const showResumeCard = task?.assignee === 'ai' && claudeResumeInfo !== null && isTerminalState;

  if (!task && !isCreateMode) return null;

  // Auto-save a patch immediately (edit mode only)
  const autoSave = async (patch: Parameters<typeof onSave>[1]) => {
    if (!task) return;
    await onSave(task.id, patch);
    void loadEvents(task.id);
  };

  const handleAddSubtask = async () => {
    const t = newSubtaskTitle.trim();
    if (!t || !task || addingSubtask) return;
    setAddingSubtask(true);
    try {
      const sub = await createSubtask({ parent_task_id: task.id, title: t });
      setSubtasks(prev => [...prev, sub]);
      setNewSubtaskTitle('');
      void loadEvents(task.id);
    } finally {
      setAddingSubtask(false);
    }
  };

  const handleDeleteSubtask = async (subtaskId: string) => {
    await deleteSubtask(subtaskId);
    setSubtasks(prev => prev.filter(s => s.id !== subtaskId));
    if (task) void loadEvents(task.id);
  };

  const handleStatusChange = async (newBucketId: string) => {
    setBucketId(newBucketId);
    if (isCreateMode) return;
    if (!task || newBucketId === task.bucket_id) return;
    await onMove(task.id, newBucketId);
    void loadEvents(task.id);
  };

  const handleTitleBlur = () => {
    if (!task || title.trim() === task.title) return;
    void autoSave({ title: title.trim() || undefined });
  };

  const handleDescriptionBlur = () => {
    if (!task || (description || null) === (task.description ?? null)) return;
    void autoSave({ description: description || null });
  };

  const handlePriorityChange = (val: number) => {
    setPriority(val);
    if (!isCreateMode) void autoSave({ priority: val });
  };

  const handleDueDateChange = (val: string) => {
    setDueDate(val);
    if (!isCreateMode) void autoSave({ due_date: val ? `${val}T00:00:00Z` : null });
  };

  const handleAssigneeChange = (val: string) => {
    setAssignee(val);
    if (!isCreateMode) void autoSave({ assignee: val || null });
  };

  const handlePickFile = async () => {
    if (!task) return;
    let absPath: string | null = null;
    try {
      absPath = await invoke<string | null>('pick_file');
    } catch (err) {
      console.error('File picker error:', err);
      return;
    }
    if (!absPath) return;
    setAttachUploading(true);
    try {
      const att = await addAttachment({ task_id: task.id, src_path: absPath, uploaded_by: 'me' });
      setAttachments(prev => [...prev, att]);
      // Refresh events so the "attached" change feed entry appears
      void loadEvents(task.id);
    } catch (err) {
      console.error('Failed to attach file:', err);
    } finally {
      setAttachUploading(false);
    }
  };

  const handleDeleteAttachment = async (attachmentId: string) => {
    if (!task) return;
    await deleteAttachment(attachmentId);
    setAttachments(prev => prev.filter(a => a.id !== attachmentId));
    void loadEvents(task.id);
  };

  // Create-mode save
  const handleCreateSave = async () => {
    if (saving) return;
    setSaving(true);
    try {
      await onCreateTask?.(bucketId, title.trim(), {
        description: description || null,
        priority: priority || undefined,
        due_date: dueDate ? `${dueDate}T00:00:00Z` : null,
        assignee: assignee || null,
      });
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const handleAddComment = async () => {
    if (!task) return;
    const body = commentDraft.trim();
    if (!body || submittingComment) return;
    setSubmittingComment(true);
    try {
      const event = await addComment(task.id, body);
      setEvents(prev => [...prev, event]);
      setCommentDraft('');
    } finally {
      setSubmittingComment(false);
    }
  };

  const commentCount = events.filter(ev => ev.kind === 'comment').length;

  const filteredEvents =
    feedFilter === 'comments' ? events.filter(ev => ev.kind === 'comment') : events;

  const TABS: { key: FeedFilter; label: string; count: number; Icon: () => React.ReactElement }[] =
    [
      {
        key: 'all',
        label: 'Activity',
        count: events.length,
        Icon: () => (
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path
              d="M3 4h10M3 8h7M3 12h5"
              stroke="currentColor"
              strokeWidth="1.5"
              strokeLinecap="round"
            />
          </svg>
        ),
      },
      {
        key: 'comments',
        label: 'Comments',
        count: commentCount,
        Icon: () => (
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path
              d="M14 10a2 2 0 01-2 2H5l-3 3V4a2 2 0 012-2h8a2 2 0 012 2v6z"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinejoin="round"
            />
          </svg>
        ),
      },
      {
        key: 'attachments',
        label: 'Files',
        count: attachments.length,
        Icon: () => (
          <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
            <path
              d="M13.5 8.5l-6 6a4 4 0 01-5.657-5.657l7-7a2.5 2.5 0 013.536 3.536l-7 7A1 1 0 014 11l6-6"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        ),
      },
    ];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm"
      onClick={onClose}>
      <div
        className="w-full max-w-5xl max-h-[90vh] bg-white dark:bg-neutral-900 rounded-xl shadow-2xl flex flex-col overflow-hidden"
        onClick={e => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-stone-200 dark:border-neutral-800 shrink-0">
          <div className="flex items-center gap-3 min-w-0">
            {/* Breadcrumb back button when viewing a subtask */}
            {parentTask && onBack && (
              <button
                type="button"
                onClick={onBack}
                className="flex items-center gap-1 text-xs text-stone-400 dark:text-neutral-500 hover:text-stone-700 dark:hover:text-neutral-300 shrink-0">
                <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
                  <path
                    d="M8 2L4 6l4 4"
                    stroke="currentColor"
                    strokeWidth="1.5"
                    strokeLinecap="round"
                    strokeLinejoin="round"
                  />
                </svg>
                <span className="truncate max-w-[100px]">{parentTask.title}</span>
              </button>
            )}
            {!isCreateMode && task && (
              <span className="text-xs font-medium text-stone-400 dark:text-neutral-500 bg-stone-100 dark:bg-neutral-800 px-2 py-0.5 rounded shrink-0">
                #{task.index}
              </span>
            )}
            <h2 className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate">
              {isCreateMode ? 'New Task' : task?.title}
            </h2>
          </div>
          <button
            type="button"
            onClick={onClose}
            className="text-stone-400 hover:text-stone-700 dark:hover:text-neutral-200 text-xl leading-none ml-4 shrink-0">
            ×
          </button>
        </div>

        {/* Two-column body */}
        <div className="flex-1 flex overflow-hidden min-h-0">
          {/* Left: task fields */}
          <div className="flex-1 min-w-0 border-r border-stone-200 dark:border-neutral-800 overflow-y-auto px-5 py-5 space-y-4">
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Status
              </label>
              <select
                value={bucketId}
                onChange={e => void handleStatusChange(e.target.value)}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100">
                {buckets.map(b => (
                  <option key={b.id} value={b.id}>
                    {b.title}
                  </option>
                ))}
              </select>{' '}
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Title
              </label>
              <input
                value={title}
                onChange={e => setTitle(e.target.value)}
                onBlur={handleTitleBlur}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Description
              </label>
              <textarea
                value={description}
                onChange={e => setDescription(e.target.value)}
                onBlur={handleDescriptionBlur}
                rows={5}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500 resize-none"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Priority
              </label>
              <select
                value={priority}
                onChange={e => handlePriorityChange(Number(e.target.value))}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100">
                {PRIORITIES.map(p => (
                  <option key={p.value} value={p.value}>
                    {p.label}
                  </option>
                ))}
              </select>
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Due date
              </label>
              <input
                type="date"
                value={dueDate}
                onChange={e => handleDueDateChange(e.target.value)}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Assignee
              </label>
              <select
                value={assignee}
                onChange={e => handleAssigneeChange(e.target.value)}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100">
                {ASSIGNEES.map(a => (
                  <option key={a.value} value={a.value}>
                    {a.label}
                  </option>
                ))}
              </select>
            </div>

            {/* Subtasks — only in edit mode, not for subtasks themselves */}
            {!isCreateMode && !task?.parent_task_id && (
              <div>
                {/* Section header */}
                <div className="flex items-center gap-2 mb-3">
                  <span className="text-sm font-semibold text-stone-800 dark:text-neutral-200">
                    Subtasks
                  </span>
                  {subtasks.length > 0 && (
                    <>
                      <div className="flex-1 h-1 rounded-full bg-stone-200 dark:bg-neutral-700 max-w-[60px]">
                        <div
                          className="h-1 rounded-full bg-stone-400 dark:bg-neutral-500 transition-all"
                          style={{
                            width: `${subtasks.length ? (subtasks.filter(s => s.done).length / subtasks.length) * 100 : 0}%`,
                          }}
                        />
                      </div>
                      <span className="text-xs text-stone-400 dark:text-neutral-500">
                        {subtasks.filter(s => s.done).length}/{subtasks.length}
                      </span>
                    </>
                  )}
                </div>

                {/* Table */}
                <div className="rounded-lg border border-stone-200 dark:border-neutral-700 overflow-hidden">
                  {/* Column headers */}
                  <div className="grid grid-cols-[1fr_32px_32px_32px_24px] items-center px-3 py-1.5 border-b border-stone-100 dark:border-neutral-800 text-[11px] text-stone-400 dark:text-neutral-500">
                    <span>Name</span>
                    <span className="text-center">
                      <svg
                        width="13"
                        height="13"
                        viewBox="0 0 16 16"
                        fill="none"
                        className="mx-auto">
                        <circle cx="8" cy="5" r="3" stroke="currentColor" strokeWidth="1.4" />
                        <path
                          d="M2 14c0-3.314 2.686-6 6-6s6 2.686 6 6"
                          stroke="currentColor"
                          strokeWidth="1.4"
                          strokeLinecap="round"
                        />
                      </svg>
                    </span>
                    <span className="text-center">
                      <svg
                        width="12"
                        height="12"
                        viewBox="0 0 16 16"
                        fill="none"
                        className="mx-auto">
                        <path
                          d="M3 2v12M3 2h8l-2 3 2 3H3"
                          stroke="currentColor"
                          strokeWidth="1.4"
                          strokeLinecap="round"
                          strokeLinejoin="round"
                        />
                      </svg>
                    </span>
                    <span className="text-center">
                      <svg
                        width="12"
                        height="12"
                        viewBox="0 0 16 16"
                        fill="none"
                        className="mx-auto">
                        <rect
                          x="1.5"
                          y="3"
                          width="13"
                          height="11"
                          rx="1.5"
                          stroke="currentColor"
                          strokeWidth="1.4"
                        />
                        <path
                          d="M5 1.5V4M11 1.5V4M1.5 6.5h13"
                          stroke="currentColor"
                          strokeWidth="1.4"
                          strokeLinecap="round"
                        />
                      </svg>
                    </span>
                    <span />
                  </div>

                  {/* Rows */}
                  {subtasks.length === 0 && (
                    <div className="px-3 py-3 text-xs text-stone-400 dark:text-neutral-500 italic">
                      No subtasks yet.
                    </div>
                  )}
                  {subtasks.map(sub => (
                    <SubtaskRow
                      key={sub.id}
                      sub={sub}
                      onOpen={() => task && onSubtaskClick(sub, task)}
                      onUpdate={async patch => {
                        await onSave(sub.id, patch);
                        void loadSubtasks(task!.id);
                        void loadEvents(task!.id);
                      }}
                      onDelete={() => void handleDeleteSubtask(sub.id)}
                    />
                  ))}

                  {/* Add Task row */}
                  <div className="flex items-center gap-2 px-3 py-2 border-t border-stone-100 dark:border-neutral-800">
                    <svg
                      width="12"
                      height="12"
                      viewBox="0 0 12 12"
                      fill="none"
                      className="shrink-0 text-stone-400 dark:text-neutral-500">
                      <path
                        d="M6 1v10M1 6h10"
                        stroke="currentColor"
                        strokeWidth="1.5"
                        strokeLinecap="round"
                      />
                    </svg>
                    <input
                      value={newSubtaskTitle}
                      onChange={e => setNewSubtaskTitle(e.target.value)}
                      onKeyDown={e => {
                        if (e.key === 'Enter') void handleAddSubtask();
                        if (e.key === 'Escape') setNewSubtaskTitle('');
                      }}
                      placeholder="Add Task"
                      className="flex-1 text-xs bg-transparent text-stone-800 dark:text-neutral-200 placeholder:text-stone-400 dark:placeholder:text-neutral-500 focus:outline-none"
                    />
                    {newSubtaskTitle.trim() && (
                      <button
                        type="button"
                        disabled={addingSubtask}
                        onClick={() => void handleAddSubtask()}
                        className="shrink-0 text-[10px] text-primary-500 hover:text-primary-600 font-medium disabled:opacity-40">
                        Add ↵
                      </button>
                    )}
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* Right: activity + attachments */}
          <div className="w-64 shrink-0 flex flex-col overflow-hidden">
            {isCreateMode ? (
              <div className="flex-1 flex items-center justify-center text-stone-300 dark:text-neutral-600 text-sm">
                Activity available after saving
              </div>
            ) : (
              <>
                {activeRun && (
                  <button
                    onClick={() => setShowRunDrawer(true)}
                    className="mb-4 w-full rounded-lg border border-stone-200 dark:border-neutral-700 overflow-hidden text-left hover:border-ocean-300 dark:hover:border-ocean-700 transition-colors">
                    <div className="flex items-center justify-between px-3 py-2 bg-stone-50 dark:bg-neutral-800">
                      <span className="text-xs font-medium text-stone-600 dark:text-neutral-300 flex items-center gap-1.5">
                        {activeRun.status === 'running' && (
                          <svg className="animate-spin h-3 w-3" viewBox="0 0 24 24" fill="none">
                            <circle
                              className="opacity-25"
                              cx="12"
                              cy="12"
                              r="10"
                              stroke="currentColor"
                              strokeWidth="4"
                            />
                            <path
                              className="opacity-75"
                              fill="currentColor"
                              d="M4 12a8 8 0 018-8v8H4z"
                            />
                          </svg>
                        )}
                        {activeRun.status === 'running'
                          ? 'AI is working…'
                          : `AI finished — ${activeRun.status}`}
                      </span>
                      <span className="text-xs text-stone-400 dark:text-neutral-500">
                        View log →
                      </span>
                    </div>
                    {activeRun.lines.at(-1) && (
                      <p className="px-3 py-1.5 text-xs font-mono text-stone-500 dark:text-neutral-400 truncate bg-white dark:bg-neutral-900 border-t border-stone-100 dark:border-neutral-800">
                        {activeRun.lines.at(-1)}
                      </p>
                    )}
                  </button>
                )}
                {showResumeCard && claudeResumeInfo && task && (
                  <ClaudeCodeResumeCard
                    sessionId={claudeResumeInfo.sessionId}
                    workspaceDir={claudeResumeInfo.workspaceDir}
                    taskId={task.id}
                  />
                )}
                {/* Tab bar — icon only with count, label as tooltip */}
                <div className="flex items-center justify-around px-2 pt-3 pb-0 border-b border-stone-200 dark:border-neutral-800 shrink-0">
                  {TABS.map(tab => (
                    <button
                      key={tab.key}
                      type="button"
                      title={tab.label}
                      onClick={() => setFeedFilter(tab.key)}
                      className={`flex flex-col items-center gap-0.5 px-3 py-1.5 border-b-2 -mb-px transition-colors ${
                        feedFilter === tab.key
                          ? 'border-primary-500 text-primary-600 dark:text-primary-400'
                          : 'border-transparent text-stone-400 dark:text-neutral-500 hover:text-stone-600 dark:hover:text-neutral-300'
                      }`}>
                      <tab.Icon />
                      {tab.count > 0 && (
                        <span
                          className={`text-[9px] font-medium leading-none ${
                            feedFilter === tab.key
                              ? 'text-primary-600 dark:text-primary-400'
                              : 'text-stone-400 dark:text-neutral-500'
                          }`}>
                          {tab.count}
                        </span>
                      )}
                    </button>
                  ))}
                </div>

                {feedFilter === 'attachments' ? (
                  /* ── Attachments tab ── */
                  <div className="flex-1 overflow-y-auto px-5 py-4">
                    {attachments.length === 0 ? (
                      <p className="text-xs text-stone-400 dark:text-neutral-500">
                        No attachments yet.
                      </p>
                    ) : (
                      <ul className="space-y-2">
                        {attachments.map(att => (
                          <li
                            key={att.id}
                            className="flex items-center gap-3 rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2.5 text-xs group">
                            <span className="text-base shrink-0">📎</span>
                            <div className="flex-1 min-w-0">
                              <p className="font-medium text-stone-800 dark:text-neutral-200 truncate">
                                {att.filename}
                              </p>
                              <p className="text-stone-400 dark:text-neutral-500 mt-0.5">
                                {formatFileSize(att.size_bytes)}
                                <span className="mx-1">·</span>
                                <span
                                  className={
                                    att.uploaded_by === 'ai'
                                      ? 'text-amber-600 dark:text-amber-400'
                                      : 'text-primary-600 dark:text-primary-400'
                                  }>
                                  by {att.uploaded_by === 'ai' ? 'AI' : 'Me'}
                                </span>
                                <span className="mx-1">·</span>
                                {formatTime(att.created)}
                              </p>
                            </div>
                            {/* Open with default app */}
                            <button
                              type="button"
                              onClick={() => void openWorkspacePath(att.rel_path)}
                              title="Open"
                              className="shrink-0 opacity-0 group-hover:opacity-100 text-stone-400 hover:text-primary-500 dark:hover:text-primary-400 transition-opacity">
                              <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                                <path
                                  d="M6 3H3a1 1 0 00-1 1v9a1 1 0 001 1h9a1 1 0 001-1v-3M10 2h4m0 0v4m0-4L7 9"
                                  stroke="currentColor"
                                  strokeWidth="1.5"
                                  strokeLinecap="round"
                                  strokeLinejoin="round"
                                />
                              </svg>
                            </button>
                            {/* Reveal in Finder */}
                            <button
                              type="button"
                              onClick={() => void revealWorkspacePath(att.rel_path)}
                              title="Show in Finder"
                              className="shrink-0 opacity-0 group-hover:opacity-100 text-stone-400 hover:text-stone-600 dark:hover:text-neutral-200 transition-opacity">
                              <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
                                <path
                                  d="M2 4a2 2 0 012-2h8a2 2 0 012 2v8a2 2 0 01-2 2H4a2 2 0 01-2-2V4z"
                                  stroke="currentColor"
                                  strokeWidth="1.4"
                                />
                                <path
                                  d="M5 8h6M8 5v6"
                                  stroke="currentColor"
                                  strokeWidth="1.4"
                                  strokeLinecap="round"
                                />
                              </svg>
                            </button>
                            {/* Delete */}
                            <button
                              type="button"
                              onClick={() => void handleDeleteAttachment(att.id)}
                              className="shrink-0 opacity-0 group-hover:opacity-100 text-stone-300 hover:text-coral-500 dark:text-neutral-600 dark:hover:text-coral-400 transition-opacity text-base leading-none"
                              title="Remove">
                              ×
                            </button>
                          </li>
                        ))}
                      </ul>
                    )}
                  </div>
                ) : (
                  /* ── Activity / Comments tab ── */
                  <div className="flex-1 overflow-y-auto px-4 py-3 space-y-1.5">
                    {eventsLoading ? (
                      <p className="text-xs text-stone-400 dark:text-neutral-500">Loading…</p>
                    ) : filteredEvents.length === 0 ? (
                      <p className="text-xs text-stone-400 dark:text-neutral-500">
                        {feedFilter === 'comments' ? 'No comments yet.' : 'No activity yet.'}
                      </p>
                    ) : (
                      filteredEvents.map(ev => (
                        <div key={ev.id}>
                          {ev.kind === 'comment' ? (
                            /* Comment — compact: actor badge + text inline, double-click to expand */
                            <div className="flex gap-2 text-xs">
                              <span
                                className={`shrink-0 mt-0.5 px-1.5 py-0.5 rounded text-[10px] font-medium self-start ${ev.actor === 'ai' ? 'bg-amber-100 text-amber-800 dark:bg-amber-500/20 dark:text-amber-300' : 'bg-primary-100 text-primary-800 dark:bg-primary-500/20 dark:text-primary-300'}`}>
                                {ev.actor === 'ai' ? 'AI' : 'Me'}
                              </span>
                              <div
                                className="flex-1 min-w-0 bg-stone-50 dark:bg-neutral-800 rounded-lg border-l-2 border-primary-400 dark:border-primary-500 px-2.5 py-1.5 cursor-pointer group"
                                onDoubleClick={() => setExpandedComment(ev.body ?? '')}
                                title="Double-click to expand">
                                <p className="text-stone-800 dark:text-neutral-200 break-words line-clamp-6">
                                  {ev.body}
                                </p>
                                <p className="text-stone-400 dark:text-neutral-500 text-[10px] mt-0.5 flex items-center gap-2">
                                  {formatTime(ev.created)}
                                  <span className="opacity-0 group-hover:opacity-60 transition-opacity">
                                    双击展开
                                  </span>
                                </p>
                              </div>
                            </div>
                          ) : (
                            /* Change event — single tight line */
                            <div className="flex gap-1.5 text-xs items-baseline">
                              <div
                                className={`shrink-0 mt-[5px] w-1 h-1 rounded-full ${
                                  ev.field === 'attachment' || ev.field === 'subtask_added'
                                    ? 'bg-primary-400 dark:bg-primary-500'
                                    : ev.field === 'attachment_removed' ||
                                        ev.field === 'subtask_removed'
                                      ? 'bg-coral-400 dark:bg-coral-500'
                                      : 'bg-stone-300 dark:bg-neutral-600'
                                }`}
                              />
                              <p className="flex-1 min-w-0 text-stone-500 dark:text-neutral-400 leading-relaxed">
                                <span
                                  className={`font-medium mr-1 ${ev.actor === 'ai' ? 'text-amber-700 dark:text-amber-400' : 'text-primary-600 dark:text-primary-400'}`}>
                                  {ev.actor === 'ai' ? 'AI' : 'Me'}
                                </span>
                                {ev.field === 'attachment' ? (
                                  <>
                                    attached{' '}
                                    <span className="font-medium text-stone-700 dark:text-neutral-300">
                                      📎 {ev.new_value}
                                    </span>
                                  </>
                                ) : ev.field === 'attachment_removed' ? (
                                  <>
                                    removed attachment{' '}
                                    <span className="line-through text-stone-400 dark:text-neutral-500">
                                      📎 {ev.old_value}
                                    </span>
                                  </>
                                ) : ev.field === 'created' ? (
                                  <>
                                    created task{' '}
                                    <span className="font-medium text-stone-700 dark:text-neutral-300">
                                      {ev.new_value}
                                    </span>
                                  </>
                                ) : ev.field === 'subtask_added' ? (
                                  <>
                                    added subtask{' '}
                                    <span className="font-medium text-stone-700 dark:text-neutral-300">
                                      {ev.new_value}
                                    </span>
                                  </>
                                ) : ev.field === 'subtask_removed' ? (
                                  <>
                                    removed subtask{' '}
                                    <span className="line-through text-stone-400">
                                      {ev.old_value}
                                    </span>
                                  </>
                                ) : ev.field === 'subtask_updated' ? (
                                  <>
                                    updated subtask{' '}
                                    <span className="font-medium text-stone-700 dark:text-neutral-300">
                                      {ev.new_value}
                                    </span>
                                  </>
                                ) : ev.old_value == null && ev.new_value == null ? (
                                  <>
                                    updated{' '}
                                    <span className="font-medium text-stone-600 dark:text-neutral-300">
                                      {FIELD_LABELS[ev.field ?? ''] ?? ev.field}
                                    </span>
                                  </>
                                ) : (
                                  <>
                                    changed{' '}
                                    <span className="font-medium text-stone-600 dark:text-neutral-300">
                                      {FIELD_LABELS[ev.field ?? ''] ?? ev.field}
                                    </span>
                                    {ev.old_value != null && (
                                      <>
                                        {' '}
                                        from{' '}
                                        <span className="line-through text-stone-400 dark:text-neutral-500">
                                          {humanizeValue(ev.field ?? '', ev.old_value, buckets)}
                                        </span>
                                      </>
                                    )}
                                    {ev.new_value != null && (
                                      <>
                                        {' '}
                                        →{' '}
                                        <span className="font-medium text-stone-700 dark:text-neutral-300">
                                          {humanizeValue(ev.field ?? '', ev.new_value, buckets)}
                                        </span>
                                      </>
                                    )}
                                  </>
                                )}
                                <span className="ml-1.5 text-stone-300 dark:text-neutral-600">
                                  {formatTime(ev.created)}
                                </span>
                              </p>
                            </div>
                          )}
                        </div>
                      ))
                    )}
                    <div ref={feedEndRef} />
                  </div>
                )}

                {/* Bottom bar: comment input + upload button */}
                <div className="px-5 py-3 border-t border-stone-200 dark:border-neutral-800 shrink-0 flex gap-2">
                  <input
                    value={commentDraft}
                    onChange={e => setCommentDraft(e.target.value)}
                    onKeyDown={e => {
                      if (e.key === 'Enter' && !e.shiftKey) {
                        e.preventDefault();
                        void handleAddComment();
                      }
                    }}
                    placeholder="Add a comment…"
                    className="flex-1 rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-xs text-stone-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500"
                  />
                  <button
                    type="button"
                    disabled={!commentDraft.trim() || submittingComment}
                    onClick={() => void handleAddComment()}
                    className="rounded-lg bg-primary-500 px-4 py-2 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-40 shrink-0">
                    Add
                  </button>
                  <button
                    type="button"
                    disabled={attachUploading}
                    onClick={() => void handlePickFile()}
                    title="Attach file"
                    className="rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-xs text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 hover:border-stone-300 dark:hover:border-neutral-600 disabled:opacity-40 shrink-0">
                    {attachUploading ? '…' : '📎'}
                  </button>
                </div>
              </>
            )}
          </div>
        </div>

        {/* Footer */}
        <div className="px-6 py-3 border-t border-stone-200 dark:border-neutral-800 flex items-center justify-between shrink-0">
          {isCreateMode ? (
            <>
              <span />
              <button
                type="button"
                disabled={saving || !title.trim()}
                onClick={() => void handleCreateSave()}
                className="rounded-lg bg-primary-500 px-5 py-2 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-50">
                {saving ? 'Creating…' : 'Create Task'}
              </button>
            </>
          ) : confirmDelete ? (
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => task && void onDelete(task.id).then(onClose)}
                className="text-xs text-coral-700 dark:text-coral-400 underline">
                Confirm delete
              </button>
              <button
                type="button"
                onClick={() => setConfirmDelete(false)}
                className="text-xs text-stone-500">
                Cancel
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={() => setConfirmDelete(true)}
              className="text-xs text-stone-400 hover:text-coral-600 dark:hover:text-coral-400">
              Delete task
            </button>
          )}
        </div>
      </div>
      {showRunDrawer && task && (
        <AiRunDrawer task={task} run={activeRun} onClose={() => setShowRunDrawer(false)} />
      )}
      {expandedComment !== null && (
        <div
          className="fixed inset-0 z-[60] flex items-center justify-center bg-black/50 backdrop-blur-sm"
          onClick={() => setExpandedComment(null)}>
          <div
            className="bg-white dark:bg-neutral-900 rounded-2xl shadow-2xl w-full max-w-2xl mx-4 max-h-[85vh] flex flex-col"
            onClick={e => e.stopPropagation()}>
            <div className="flex items-center justify-between px-5 py-3 border-b border-stone-200 dark:border-neutral-700">
              <span className="text-sm font-semibold text-stone-700 dark:text-neutral-200">
                Comment
              </span>
              <button
                onClick={() => setExpandedComment(null)}
                className="text-stone-400 hover:text-stone-600 dark:hover:text-neutral-200 text-xl leading-none px-1">
                ×
              </button>
            </div>
            <div className="flex-1 overflow-y-auto px-5 py-4">
              <BubbleMarkdown content={expandedComment} tone="agent" />
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// SubtaskRow
// ---------------------------------------------------------------------------

function SubtaskRow({
  sub,
  onOpen,
  onUpdate,
  onDelete,
}: {
  sub: Task;
  onOpen: () => void;
  onUpdate: (patch: {
    title?: string;
    assignee?: string | null;
    priority?: number;
    due_date?: string | null;
    done?: boolean;
  }) => Promise<void>;
  onDelete: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [titleDraft, setTitleDraft] = useState(sub.title);
  const inputRef = useRef<HTMLInputElement>(null);

  const startEdit = () => {
    setTitleDraft(sub.title);
    setEditing(true);
    setTimeout(() => inputRef.current?.focus(), 0);
  };

  const commitEdit = () => {
    const t = titleDraft.trim();
    setEditing(false);
    if (t && t !== sub.title) void onUpdate({ title: t });
    else setTitleDraft(sub.title);
  };

  return (
    <div className="grid grid-cols-[1fr_32px_32px_32px_24px] items-center px-3 py-2 border-t border-stone-100 dark:border-neutral-800 hover:bg-stone-50 dark:hover:bg-neutral-800/40 group text-xs first:border-t-0">
      {/* Name cell */}
      <div className="flex items-center gap-2 min-w-0">
        {/* Done circle — dashed when incomplete */}
        <button
          type="button"
          onClick={() => void onUpdate({ done: !sub.done })}
          className="shrink-0">
          {sub.done ? (
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
              <circle cx="8" cy="8" r="7" fill="#22c55e" />
              <path
                d="M5 8l2.5 2.5L11 5.5"
                stroke="white"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          ) : (
            <svg width="15" height="15" viewBox="0 0 16 16" fill="none">
              <circle
                cx="8"
                cy="8"
                r="6.5"
                stroke="#d1d5db"
                strokeWidth="1.4"
                strokeDasharray="3 2"
              />
            </svg>
          )}
        </button>

        {/* Title */}
        {editing ? (
          <input
            ref={inputRef}
            value={titleDraft}
            onChange={e => setTitleDraft(e.target.value)}
            onBlur={commitEdit}
            onKeyDown={e => {
              if (e.key === 'Enter') commitEdit();
              if (e.key === 'Escape') {
                setEditing(false);
                setTitleDraft(sub.title);
              }
            }}
            className="flex-1 min-w-0 bg-white dark:bg-neutral-800 border border-primary-400 rounded px-1.5 py-0.5 text-xs text-stone-900 dark:text-neutral-100 focus:outline-none"
          />
        ) : (
          <div className="flex items-center gap-1.5 flex-1 min-w-0">
            <button
              type="button"
              onClick={onOpen}
              className={`flex-1 min-w-0 text-left truncate text-sm font-medium ${
                sub.done
                  ? 'line-through text-stone-400 dark:text-neutral-500'
                  : 'text-stone-800 dark:text-neutral-100'
              }`}>
              {sub.title}
            </button>
            {/* Hover actions: rename + delete */}
            <div className="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity shrink-0">
              <button
                type="button"
                onClick={startEdit}
                title="Rename"
                className="p-0.5 rounded text-stone-400 hover:text-stone-600 hover:bg-stone-200 dark:hover:bg-neutral-700 transition-colors">
                <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
                  <path
                    d="M8.5 1.5l2 2L4 10H2v-2L8.5 1.5z"
                    stroke="currentColor"
                    strokeWidth="1.3"
                    strokeLinejoin="round"
                  />
                </svg>
              </button>
            </div>
          </div>
        )}
      </div>

      {/* Assignee — icon with hidden select overlay */}
      <div className="relative flex items-center justify-center">
        {sub.assignee ? (
          <div className="w-6 h-6 rounded-full bg-stone-600 dark:bg-neutral-500 flex items-center justify-center">
            <span className="text-[9px] font-bold text-white leading-none pointer-events-none">
              {sub.assignee === 'ai' ? 'AI' : 'ME'}
            </span>
          </div>
        ) : (
          <svg
            width="14"
            height="14"
            viewBox="0 0 16 16"
            fill="none"
            className="text-stone-300 dark:text-neutral-600 pointer-events-none">
            <circle cx="8" cy="5.5" r="2.5" stroke="currentColor" strokeWidth="1.3" />
            <path
              d="M2 14.5c0-3.038 2.686-5.5 6-5.5"
              stroke="currentColor"
              strokeWidth="1.3"
              strokeLinecap="round"
            />
            <path
              d="M11 12v4M13 14h-4"
              stroke="currentColor"
              strokeWidth="1.3"
              strokeLinecap="round"
            />
          </svg>
        )}
        <select
          value={sub.assignee ?? ''}
          onChange={e => void onUpdate({ assignee: e.target.value || null })}
          title="Assignee"
          className="absolute inset-0 opacity-0 cursor-pointer w-full h-full">
          <option value="">— Unassigned</option>
          <option value="me">Me</option>
          <option value="ai">AI</option>
        </select>
      </div>

      {/* Priority — icon with hidden select overlay */}
      <div className="relative flex items-center justify-center">
        {sub.priority > 0 ? (
          <svg
            width="12"
            height="12"
            viewBox="0 0 16 16"
            fill="none"
            className={`pointer-events-none ${
              sub.priority >= 4
                ? 'text-coral-500'
                : sub.priority === 3
                  ? 'text-amber-500'
                  : 'text-primary-400'
            }`}>
            <path
              d="M3 2v12M3 2h8l-2 3 2 3H3"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        ) : (
          <svg
            width="12"
            height="12"
            viewBox="0 0 16 16"
            fill="none"
            className="text-stone-300 dark:text-neutral-600 pointer-events-none">
            <path
              d="M3 2v12M3 2h8l-2 3 2 3H3"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        )}
        <select
          value={sub.priority}
          onChange={e => void onUpdate({ priority: Number(e.target.value) })}
          title="Priority"
          className="absolute inset-0 opacity-0 cursor-pointer w-full h-full">
          <option value={0}>— None</option>
          <option value={1}>Low</option>
          <option value={2}>Medium</option>
          <option value={3}>High</option>
          <option value={4}>Urgent</option>
          <option value={5}>Critical</option>
        </select>
      </div>

      {/* Due date — icon with hidden date input overlay */}
      <div className="relative flex items-center justify-center">
        {sub.due_date ? (
          <span className="text-[9px] text-amber-600 dark:text-amber-400 font-medium pointer-events-none leading-none">
            {new Date(sub.due_date).toLocaleDateString(undefined, {
              month: 'short',
              day: 'numeric',
            })}
          </span>
        ) : (
          <svg
            width="12"
            height="12"
            viewBox="0 0 16 16"
            fill="none"
            className="text-stone-300 dark:text-neutral-600 pointer-events-none">
            <rect
              x="1.5"
              y="3"
              width="13"
              height="11"
              rx="1.5"
              stroke="currentColor"
              strokeWidth="1.4"
            />
            <path
              d="M5 1.5V4M11 1.5V4M1.5 6.5h13"
              stroke="currentColor"
              strokeWidth="1.4"
              strokeLinecap="round"
            />
          </svg>
        )}
        <input
          type="date"
          value={sub.due_date ? sub.due_date.slice(0, 10) : ''}
          onChange={e =>
            void onUpdate({ due_date: e.target.value ? `${e.target.value}T00:00:00Z` : null })
          }
          title="Due date"
          className="absolute inset-0 opacity-0 cursor-pointer w-full h-full"
        />
      </div>

      {/* Delete */}
      <div className="flex items-center justify-center">
        <button
          type="button"
          onClick={onDelete}
          title="Delete"
          className="opacity-0 group-hover:opacity-40 hover:!opacity-100 text-stone-400 hover:text-coral-500 dark:hover:text-coral-400 transition-opacity text-sm leading-none">
          ×
        </button>
      </div>
    </div>
  );
}
