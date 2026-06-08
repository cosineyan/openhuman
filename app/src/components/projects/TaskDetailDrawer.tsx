import { invoke } from '@tauri-apps/api/core';
import { useCallback, useEffect, useRef, useState } from 'react';

import { formatFileSize } from '../../lib/attachments';
import {
  addAttachment,
  addComment,
  deleteAttachment,
  listAttachments,
  listTaskEvents,
  type Bucket,
  type Task,
  type TaskAttachment,
  type TaskEvent,
} from '../../services/api/projectsApi';

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
  onClose: () => void;
  onSave: (taskId: string, patch: SavePatch) => Promise<void>;
  onDelete: (taskId: string) => Promise<void>;
  onMove: (taskId: string, bucketId: string) => Promise<void>;
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
  { value: 'ai', label: 'AI (Wukong)' },
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
  onClose,
  onSave,
  onDelete,
  onMove,
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

  const [events, setEvents] = useState<TaskEvent[]>([]);
  const [eventsLoading, setEventsLoading] = useState(false);
  const [feedFilter, setFeedFilter] = useState<FeedFilter>('all');
  const [commentDraft, setCommentDraft] = useState('');
  const [submittingComment, setSubmittingComment] = useState(false);
  const feedEndRef = useRef<HTMLDivElement>(null);

  const [attachments, setAttachments] = useState<TaskAttachment[]>([]);
  const [attachUploading, setAttachUploading] = useState(false);

  const loadEvents = useCallback(async (taskId: string) => {
    setEventsLoading(true);
    try {
      const data = await listTaskEvents(taskId);
      setEvents(data);
    } finally {
      setEventsLoading(false);
    }
  }, []);

  const loadAttachments = useCallback(async (taskId: string) => {
    try {
      const data = await listAttachments(taskId);
      setAttachments(data);
    } catch {
      // non-fatal
    }
  }, []);

  useEffect(() => {
    if (task) {
      setTitle(task.title);
      setDescription(task.description ?? '');
      setPriority(task.priority);
      setDueDate(task.due_date ? task.due_date.slice(0, 10) : '');
      setAssignee(task.assignee ?? '');
      setBucketId(task.bucket_id);
      setConfirmDelete(false);
      setCommentDraft('');
      void loadEvents(task.id);
      void loadAttachments(task.id);
    } else if (createBucketId) {
      setTitle('');
      setDescription('');
      setPriority(0);
      setDueDate('');
      setAssignee('');
      setBucketId(createBucketId);
      setConfirmDelete(false);
    }
  }, [task, createBucketId, loadEvents, loadAttachments]);

  useEffect(() => {
    if (feedFilter !== 'attachments') {
      feedEndRef.current?.scrollIntoView({ behavior: 'smooth' });
    }
  }, [events, feedFilter]);

  if (!task && !isCreateMode) return null;

  const handlePickFile = async () => {
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
    await deleteAttachment(attachmentId);
    setAttachments(prev => prev.filter(a => a.id !== attachmentId));
    void loadEvents(task.id);
  };

  const handleSave = async () => {
    if (saving) return;
    setSaving(true);
    try {
      if (isCreateMode) {
        await onCreateTask?.(bucketId, title.trim(), {
          description: description || null,
          priority: priority || undefined,
          due_date: dueDate ? `${dueDate}T00:00:00Z` : null,
          assignee: assignee || null,
        });
      } else if (task) {
        // Move bucket first if it changed, then patch other fields
        if (bucketId !== task.bucket_id) {
          await onMove(task.id, bucketId);
        }
        await onSave(task.id, {
          title: title.trim() || undefined,
          description: description || null,
          priority,
          due_date: dueDate ? `${dueDate}T00:00:00Z` : null,
          assignee: assignee || null,
        });
      }
      onClose();
    } finally {
      setSaving(false);
    }
  };

  const handleStatusChange = (newBucketId: string) => {
    setBucketId(newBucketId);
  };

  const handleAddComment = async () => {
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

  const TABS: { key: FeedFilter; label: string; count: number }[] = [
    { key: 'all', label: 'All activity', count: events.length },
    { key: 'comments', label: 'Comments', count: commentCount },
    { key: 'attachments', label: 'Attachments', count: attachments.length },
  ];

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-sm"
      onClick={onClose}>
      <div
        className="w-full max-w-4xl max-h-[90vh] bg-white dark:bg-neutral-900 rounded-xl shadow-2xl flex flex-col overflow-hidden"
        onClick={e => e.stopPropagation()}>
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-stone-200 dark:border-neutral-800 shrink-0">
          <div className="flex items-center gap-3">
            {!isCreateMode && task && (
              <span className="text-xs font-medium text-stone-400 dark:text-neutral-500 bg-stone-100 dark:bg-neutral-800 px-2 py-0.5 rounded">
                #{task.index}
              </span>
            )}
            <h2 className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate max-w-sm">
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
          <div className="w-80 shrink-0 border-r border-stone-200 dark:border-neutral-800 overflow-y-auto px-5 py-5 space-y-4">
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
              </select>
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Title
              </label>
              <input
                value={title}
                onChange={e => setTitle(e.target.value)}
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
                onChange={e => setPriority(Number(e.target.value))}
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
                onChange={e => setDueDate(e.target.value)}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100"
              />
            </div>
            <div>
              <label className="text-xs font-medium text-stone-500 dark:text-neutral-400 block mb-1">
                Assignee
              </label>
              <select
                value={assignee}
                onChange={e => setAssignee(e.target.value)}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100">
                {ASSIGNEES.map(a => (
                  <option key={a.value} value={a.value}>
                    {a.label}
                  </option>
                ))}
              </select>
            </div>
          </div>

          {/* Right: activity + attachments */}
          <div className="flex-1 flex flex-col overflow-hidden">
            {isCreateMode ? (
              <div className="flex-1 flex items-center justify-center text-stone-300 dark:text-neutral-600 text-sm">
                Activity available after saving
              </div>
            ) : (
              <>
                {/* Tab bar */}
                <div className="flex items-center gap-1 px-5 pt-4 pb-0 border-b border-stone-200 dark:border-neutral-800 shrink-0">
                  {TABS.map(tab => (
                    <button
                      key={tab.key}
                      type="button"
                      onClick={() => setFeedFilter(tab.key)}
                      className={`flex items-center gap-1.5 px-3 py-2 text-xs font-medium border-b-2 -mb-px transition-colors ${
                        feedFilter === tab.key
                          ? 'border-primary-500 text-primary-600 dark:text-primary-400'
                          : 'border-transparent text-stone-500 dark:text-neutral-500 hover:text-stone-700 dark:hover:text-neutral-300'
                      }`}>
                      {tab.label}
                      <span
                        className={`rounded-full px-1.5 py-0.5 text-[10px] font-medium ${
                          feedFilter === tab.key
                            ? 'bg-primary-100 text-primary-700 dark:bg-primary-500/20 dark:text-primary-300'
                            : 'bg-stone-100 text-stone-500 dark:bg-neutral-800 dark:text-neutral-500'
                        }`}>
                        {tab.count}
                      </span>
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
                            className="flex items-center gap-3 rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2.5 text-xs">
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
                            <button
                              type="button"
                              onClick={() => void handleDeleteAttachment(att.id)}
                              className="shrink-0 text-stone-300 hover:text-coral-500 dark:text-neutral-600 dark:hover:text-coral-400 text-base leading-none"
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
                  <div className="flex-1 overflow-y-auto px-5 py-4 space-y-3">
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
                            <div className="flex gap-2.5 text-xs">
                              <span
                                className={`shrink-0 mt-0.5 px-1.5 py-0.5 rounded font-medium self-start ${ev.actor === 'ai' ? 'bg-amber-100 text-amber-800 dark:bg-amber-500/20 dark:text-amber-300' : 'bg-primary-100 text-primary-800 dark:bg-primary-500/20 dark:text-primary-300'}`}>
                                {ev.actor === 'ai' ? 'AI' : 'Me'}
                              </span>
                              <div className="flex-1 min-w-0 rounded-lg border border-stone-200 dark:border-neutral-700 border-l-2 border-l-primary-400 dark:border-l-primary-500 bg-stone-50 dark:bg-neutral-800 px-3 py-2.5">
                                <p className="text-stone-800 dark:text-neutral-200 break-words leading-relaxed">
                                  {ev.body}
                                </p>
                                <p className="text-stone-400 dark:text-neutral-500 mt-1.5">
                                  {formatTime(ev.created)}
                                </p>
                              </div>
                            </div>
                          ) : (
                            <div className="flex gap-2 text-xs items-start">
                              <div
                                className={`shrink-0 mt-[5px] w-1.5 h-1.5 rounded-full ${
                                  ev.field === 'attachment'
                                    ? 'bg-primary-300 dark:bg-primary-600'
                                    : ev.field === 'attachment_removed'
                                      ? 'bg-coral-300 dark:bg-coral-600'
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
            <span />
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
          <button
            type="button"
            disabled={saving}
            onClick={() => void handleSave()}
            className="rounded-lg bg-primary-500 px-5 py-2 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-50">
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
}
