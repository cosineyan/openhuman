import { useState, useEffect } from 'react';
import type { Task } from '../../services/api/projectsApi';

interface SavePatch {
  title?: string;
  description?: string | null;
  priority?: number;
  due_date?: string | null;
  assignee?: string | null;
}

interface Props {
  task: Task | null;
  onClose: () => void;
  onSave: (taskId: string, patch: SavePatch) => Promise<void>;
  onDelete: (taskId: string) => Promise<void>;
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

export function TaskDetailDrawer({ task, onClose, onSave, onDelete }: Props) {
  const [title, setTitle] = useState('');
  const [description, setDescription] = useState('');
  const [priority, setPriority] = useState(0);
  const [dueDate, setDueDate] = useState('');
  const [assignee, setAssignee] = useState('');
  const [saving, setSaving] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);

  useEffect(() => {
    if (task) {
      setTitle(task.title);
      setDescription(task.description ?? '');
      setPriority(task.priority);
      setDueDate(task.due_date ? task.due_date.slice(0, 10) : '');
      setAssignee(task.assignee ?? '');
      setConfirmDelete(false);
    }
  }, [task]);

  if (!task) return null;

  const handleSave = async () => {
    if (saving) return;
    setSaving(true);
    try {
      await onSave(task.id, {
        title: title.trim() || undefined,
        description: description || null,
        priority,
        due_date: dueDate || null,
        assignee: assignee || null,
      });
      onClose();
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="fixed inset-0 z-40 flex" onClick={onClose}>
      <div className="flex-1" />
      <div
        className="w-full max-w-sm h-full bg-white dark:bg-neutral-900 border-l border-stone-200 dark:border-neutral-800 flex flex-col shadow-xl"
        onClick={e => e.stopPropagation()}
      >
        {/* Header */}
        <div className="flex items-center justify-between px-4 py-3 border-b border-stone-200 dark:border-neutral-800">
          <span className="text-xs text-stone-400 dark:text-neutral-500">Task #{task.index}</span>
          <button type="button" onClick={onClose} className="text-stone-400 hover:text-stone-700 dark:hover:text-neutral-200 text-lg leading-none">×</button>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-y-auto px-4 py-4 space-y-4">
          <div>
            <label className="text-xs font-medium text-stone-600 dark:text-neutral-300 block mb-1">Title</label>
            <input
              value={title}
              onChange={e => setTitle(e.target.value)}
              className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500"
            />
          </div>

          <div>
            <label className="text-xs font-medium text-stone-600 dark:text-neutral-300 block mb-1">Description</label>
            <textarea
              value={description}
              onChange={e => setDescription(e.target.value)}
              rows={4}
              className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500 resize-none"
            />
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="text-xs font-medium text-stone-600 dark:text-neutral-300 block mb-1">Priority</label>
              <select
                value={priority}
                onChange={e => setPriority(Number(e.target.value))}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100"
              >
                {PRIORITIES.map(p => (
                  <option key={p.value} value={p.value}>{p.label}</option>
                ))}
              </select>
            </div>
            <div>
              <label className="text-xs font-medium text-stone-600 dark:text-neutral-300 block mb-1">Due date</label>
              <input
                type="date"
                value={dueDate}
                onChange={e => setDueDate(e.target.value)}
                className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100"
              />
            </div>
          </div>

          <div>
            <label className="text-xs font-medium text-stone-600 dark:text-neutral-300 block mb-1">Assignee</label>
            <select
              value={assignee}
              onChange={e => setAssignee(e.target.value)}
              className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100"
            >
              {ASSIGNEES.map(a => (
                <option key={a.value} value={a.value}>{a.label}</option>
              ))}
            </select>
          </div>
        </div>

        {/* Footer */}
        <div className="px-4 py-3 border-t border-stone-200 dark:border-neutral-800 flex items-center justify-between">
          {confirmDelete ? (
            <div className="flex gap-2">
              <button type="button" onClick={() => void onDelete(task.id).then(onClose)} className="text-xs text-coral-700 dark:text-coral-400 underline">Confirm delete</button>
              <button type="button" onClick={() => setConfirmDelete(false)} className="text-xs text-stone-500">Cancel</button>
            </div>
          ) : (
            <button type="button" onClick={() => setConfirmDelete(true)} className="text-xs text-stone-400 hover:text-coral-600 dark:hover:text-coral-400">Delete</button>
          )}
          <button
            type="button"
            disabled={saving}
            onClick={() => void handleSave()}
            className="rounded-lg bg-primary-500 px-4 py-2 text-xs font-medium text-white hover:bg-primary-600 disabled:opacity-50"
          >
            {saving ? 'Saving…' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  );
}
