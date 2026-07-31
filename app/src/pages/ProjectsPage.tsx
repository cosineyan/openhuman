import { useCallback, useEffect, useState } from 'react';
import { useLocation } from 'react-router-dom';

import { SidebarContent } from '../components/layout/shell/SidebarSlot';
import TwoPaneNav from '../components/layout/TwoPaneNav';
import { ArchivedView } from '../components/projects/ArchivedView';
import { EmailAutomationPanel } from '../components/projects/EmailAutomationPanel';
import { ScheduledTaskPanel } from '../components/projects/ScheduledTaskPanel';
import { KanbanBoard } from '../components/projects/KanbanBoard';
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

type ViewMode = 'board' | 'archived' | 'email_automation' | 'scheduled_tasks';

function NavIcon({ path }: { path: string }) {
  return (
    <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
      <path d={path} />
    </svg>
  );
}

const BOARD_ICON_PATH = 'M9 3H5a2 2 0 00-2 2v4m6-6h10a2 2 0 012 2v4M9 3v18m0 0h10a2 2 0 002-2V9M9 21H5a2 2 0 01-2-2V9m0 0h18';
const ARCHIVED_ICON_PATH = 'M5 8h14M5 8a2 2 0 110-4h14a2 2 0 110 4M5 8v10a2 2 0 002 2h10a2 2 0 002-2V8m-9 4h4';
const EMAIL_ICON_PATH = 'M3 8l7.89 5.26a2 2 0 002.22 0L21 8M5 19h14a2 2 0 002-2V7a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z';

export function ProjectsPage() {
  const { t } = useT();
  const { pathname } = useLocation();
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

  // Reload when the window regains focus (user navigates away and comes back).
  useEffect(() => {
    const onFocus = () => void reload();
    window.addEventListener('focus', onFocus);
    return () => window.removeEventListener('focus', onFocus);
  }, [reload]);

  // Reload when navigating back to this page via route change.
  useEffect(() => {
    void reload();
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pathname]);

  // Poll every 5s while any task with assignee=ai is in a non-terminal bucket
  // (i.e. AI may be actively working). This keeps the board + open drawer fresh.
  useEffect(() => {
    const hasActiveAiTask = board?.buckets.some(
      b => !b.bucket.is_done_bucket && b.tasks.some(t => t.assignee === 'ai' && !t.done)
    );
    if (!hasActiveAiTask) return;
    const interval = setInterval(() => {
      void reload();
    }, 5000);
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
                    {
                      ...task,
                      bucket_id: destBucketId,
                      position,
                      done: destBucket.bucket.is_done_bucket,
                    },
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
    <div className="flex h-full">
      {/* Left sidebar */}
      <SidebarContent>
        <TwoPaneNav
          ariaLabel="Projects navigation"
          selected={viewMode}
          onSelect={value => setViewMode(value as ViewMode)}
          header={
            <div className="px-1 pb-2 pt-1">
              <h2 className="text-sm font-semibold text-stone-800 dark:text-neutral-100">
                {board.project.title}
              </h2>
            </div>
          }
          groups={[
            {
              label: 'Views',
              items: [
                { value: 'board', label: 'Board', icon: <NavIcon path={BOARD_ICON_PATH} /> },
                { value: 'archived', label: 'Archived', icon: <NavIcon path={ARCHIVED_ICON_PATH} /> },
              ],
            },
            {
              label: 'Automation',
              items: [
                { value: 'email_automation', label: 'Email → Task', icon: <NavIcon path={EMAIL_ICON_PATH} /> },
                { value: 'scheduled_tasks', label: 'Scheduling Task', icon: <NavIcon path={EMAIL_ICON_PATH} /> },
              ],
            },
          ]}
        />
      </SidebarContent>

      {/* Main content — flex-1 to fill remaining space after sidebar */}
      <div className="flex-1 min-w-0 flex flex-col h-full overflow-hidden p-4">
        {viewMode === 'board' && (
          <div className="h-full overflow-hidden rounded-2xl border border-stone-200 bg-white shadow-soft dark:border-neutral-800 dark:bg-neutral-900">
            <div className="flex-1 min-h-0 overflow-auto p-4 h-full">
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
          </div>
        )}
        {viewMode === 'archived' && <ArchivedView onTaskClick={setSelectedTask} />}
        {viewMode === 'email_automation' && (
          <EmailAutomationPanel
            onOpenTask={(taskId) => {
              const task = board?.buckets.flatMap(b => b.tasks).find(t => t.id === taskId);
              if (task) {
                setSelectedTask(task);
                setTaskStack([]);
              }
            }}
          />
        )}
        {viewMode === 'scheduled_tasks' && (
          <ScheduledTaskPanel
            onOpenTask={(title) => {
              const task = board?.buckets.flatMap(b => b.tasks).find(t => t.title === title);
              if (task) { setSelectedTask(task); setTaskStack([]); }
            }}
          />
        )}
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
        onBack={
          taskStack.length > 0
            ? () => {
                const parent = taskStack[taskStack.length - 1];
                setTaskStack(prev => prev.slice(0, -1));
                setSelectedTask(parent);
              }
            : undefined
        }
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
