import { useCallback, useEffect, useState } from 'react';

import { KanbanBoard } from '../components/projects/KanbanBoard';
import { ListView } from '../components/projects/ListView';
import { TableView } from '../components/projects/TableView';
import { TaskDetailDrawer } from '../components/projects/TaskDetailDrawer';
import { useT } from '../lib/i18n/I18nContext';
import {
  type BoardData,
  createTask,
  deleteTask,
  getBoard,
  moveTask,
  type Task,
  updateBucket,
  updateTask,
} from '../services/api/projectsApi';

type ViewMode = 'board' | 'list' | 'table';

function BoardIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
      <rect x="1" y="1" width="6" height="14" rx="1.5" stroke="currentColor" strokeWidth="1.4" />
      <rect x="9" y="1" width="6" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}
function ListIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
      <path
        d="M1 4h14M1 8h14M1 12h14"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}
function TableIcon() {
  return (
    <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
      <rect x="1" y="1" width="14" height="14" rx="1.5" stroke="currentColor" strokeWidth="1.4" />
      <path d="M1 5.5h14M6 5.5v9.5" stroke="currentColor" strokeWidth="1.2" />
    </svg>
  );
}

const VIEWS: { key: ViewMode; label: string; Icon: () => React.ReactElement }[] = [
  { key: 'board', label: 'Board', Icon: BoardIcon },
  { key: 'list', label: 'List', Icon: ListIcon },
  { key: 'table', label: 'Table', Icon: TableIcon },
];

export function ProjectsPage() {
  const { t } = useT();
  const [board, setBoard] = useState<BoardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);
  const [taskStack, setTaskStack] = useState<Task[]>([]);
  const [createBucketId, setCreateBucketId] = useState<string | null>(null);
  const [viewMode, setViewMode] = useState<ViewMode>('board');
  const [boardVersion, setBoardVersion] = useState(0);

  const reload = useCallback(async () => {
    try {
      const data = await getBoard();
      setBoard(data);
      setError(null);
      setBoardVersion(v => v + 1);
      // Keep selectedTask in sync: if the drawer is open, update it with
      // the fresh task data so the feed + status reflect external changes
      // (e.g. AI moving the task, or a drag while the drawer is open).
      setSelectedTask(prev => {
        if (!prev) return prev;
        const fresh = data.buckets.flatMap(b => b.tasks).find(t => t.id === prev.id);
        return fresh ?? prev;
      });
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  // Poll every 5s while any task with assignee=ai is in a non-terminal bucket
  // (i.e. AI may be actively working). This keeps the board + open drawer fresh.
  useEffect(() => {
    const hasActiveAiTask = board?.buckets.some(b =>
      !b.bucket.is_done_bucket &&
      b.tasks.some(t => t.assignee === 'ai' && !t.done)
    );
    if (!hasActiveAiTask) return;
    const interval = setInterval(() => { void reload(); }, 5000);
    return () => clearInterval(interval);
  }, [board, reload]);

  const handleAddTask = async (
    bucketId: string,
    title: string,
    opts?: { assignee?: string; due_date?: string; priority?: number }
  ) => {
    const task = await createTask({
      title,
      bucket_id: bucketId,
      priority: opts?.priority,
      due_date: opts?.due_date ? `${opts.due_date}T00:00:00Z` : undefined,
    });
    if (opts?.assignee) {
      await updateTask({ task_id: task.id, patch: { assignee: opts.assignee } });
    }
    await reload();
  };

  const handleCreateTaskFromModal = async (
    bucketId: string,
    title: string,
    patch: {
      description?: string | null;
      priority?: number;
      due_date?: string | null;
      assignee?: string | null;
    }
  ) => {
    const task = await createTask({
      title,
      bucket_id: bucketId,
      priority: patch.priority,
      due_date: patch.due_date ?? undefined,
    });
    const extraPatch: Parameters<typeof updateTask>[0]['patch'] = {};
    if (patch.assignee !== undefined) extraPatch.assignee = patch.assignee;
    if (patch.description !== undefined) extraPatch.description = patch.description;
    if (Object.keys(extraPatch).length > 0) {
      await updateTask({ task_id: task.id, patch: extraPatch });
    }
    setCreateBucketId(null);
    await reload();
  };

  const handleMoveTask = async (taskId: string, destBucketId: string, destIndex: number) => {
    if (!board) return;
    const destBucket = board.buckets.find(b => b.bucket.id === destBucketId);
    if (!destBucket) return;
    const tasks = destBucket.tasks;
    const before = tasks[destIndex - 1]?.position ?? 0;
    const after = tasks[destIndex]?.position ?? before + 2000;
    const position = (before + after) / 2;

    setBoard(prev => {
      if (!prev) return prev;
      const allTasks = prev.buckets.flatMap(b => b.tasks);
      const task = allTasks.find(t => t.id === taskId);
      if (!task) return prev;
      return {
        ...prev,
        buckets: prev.buckets
          .map(bwt => ({ ...bwt, tasks: bwt.tasks.filter(t => t.id !== taskId) }))
          .map(bwt =>
            bwt.bucket.id === destBucketId
              ? {
                  ...bwt,
                  tasks: [
                    ...bwt.tasks.slice(0, destIndex),
                    { ...task, bucket_id: destBucketId, position },
                    ...bwt.tasks.slice(destIndex),
                  ],
                }
              : bwt
          ),
      };
    });

    try {
      await moveTask({ task_id: taskId, bucket_id: destBucketId, position });
    } catch {
      await reload();
    }
  };

  const handleSaveTask = async (
    taskId: string,
    patch: Parameters<typeof updateTask>[0]['patch']
  ) => {
    await updateTask({ task_id: taskId, patch });
    await reload();
    setSelectedTask(prev => (!prev || prev.id !== taskId ? prev : prev));
  };

  const handleMoveTaskFromDrawer = async (taskId: string, bucketId: string) => {
    await moveTask({ task_id: taskId, bucket_id: bucketId });
    await reload();
  };

  const handleDeleteTask = async (taskId: string) => {
    await deleteTask(taskId);
    setSelectedTask(null);
    await reload();
  };

  const handleRenameColumn = async (bucketId: string, title: string) => {
    await updateBucket({ bucket_id: bucketId, patch: { title } });
    await reload();
  };

  if (loading) {
    return (
      <div className="flex items-center justify-center h-full text-stone-400 dark:text-neutral-500 text-sm">
        {t('common.loading')}
      </div>
    );
  }

  if (error) {
    return <div className="p-4 text-sm text-coral-700 dark:text-coral-400">{error}</div>;
  }

  if (!board) return null;

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-4 pt-3 border-b border-stone-200 dark:border-neutral-800 shrink-0">
        <h1 className="text-base font-semibold text-stone-900 dark:text-neutral-100 mb-2.5">
          {board.project.title}
        </h1>
        {/* View tabs */}
        <div className="flex items-center gap-1">
          {VIEWS.map(({ key, label, Icon }) => (
            <button
              key={key}
              type="button"
              onClick={() => setViewMode(key)}
              className={`flex items-center gap-1.5 px-3 py-1.5 text-xs font-medium rounded-t-md border-b-2 -mb-px transition-colors ${
                viewMode === key
                  ? 'border-primary-500 text-primary-600 dark:text-primary-400 bg-white dark:bg-neutral-950'
                  : 'border-transparent text-stone-500 dark:text-neutral-500 hover:text-stone-700 dark:hover:text-neutral-300'
              }`}>
              <Icon />
              {label}
            </button>
          ))}
        </div>
      </div>

      {/* Content */}
      <div
        className={`flex-1 min-h-0 ${viewMode === 'board' ? 'overflow-auto p-4' : 'overflow-auto p-4'}`}>
        {viewMode === 'board' && (
          <div className="h-full">
            <KanbanBoard
              board={board}
              onTaskClick={setSelectedTask}
              onAddTask={handleAddTask}
              onAddViaModal={setCreateBucketId}
              onMoveTask={handleMoveTask}
              onRenameColumn={handleRenameColumn}
              boardVersion={boardVersion}
            />
          </div>
        )}
        {viewMode === 'list' && <ListView board={board} onTaskClick={setSelectedTask} />}
        {viewMode === 'table' && <TableView board={board} onTaskClick={setSelectedTask} />}
      </div>

      <TaskDetailDrawer
        task={selectedTask}
        buckets={board.buckets.map(b => b.bucket)}
        parentTask={taskStack.length > 0 ? taskStack[taskStack.length - 1] : null}
        onClose={() => {
          setSelectedTask(null);
          setTaskStack([]);
          setCreateBucketId(null);
        }}
        onBack={taskStack.length > 0 ? () => {
          const parent = taskStack[taskStack.length - 1];
          setTaskStack(prev => prev.slice(0, -1));
          setSelectedTask(parent);
        } : undefined}
        onSave={handleSaveTask}
        onDelete={handleDeleteTask}
        onMove={handleMoveTaskFromDrawer}
        onSubtaskClick={(subtask, parent) => {
          setTaskStack(prev => [...prev, parent]);
          setSelectedTask(subtask);
        }}
        createBucketId={createBucketId}
        onCreateTask={handleCreateTaskFromModal}
      />
    </div>
  );
}
