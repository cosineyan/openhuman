import { useCallback, useEffect, useState } from 'react';

import { KanbanBoard } from '../components/projects/KanbanBoard';
import { TaskDetailDrawer } from '../components/projects/TaskDetailDrawer';
import { useT } from '../lib/i18n/I18nContext';
import {
  createTask,
  deleteTask,
  getBoard,
  moveTask,
  updateBucket,
  updateTask,
  type BoardData,
  type Task,
} from '../services/api/projectsApi';

export function ProjectsPage() {
  const { t } = useT();
  const [board, setBoard] = useState<BoardData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [selectedTask, setSelectedTask] = useState<Task | null>(null);

  const reload = useCallback(async () => {
    try {
      const data = await getBoard();
      setBoard(data);
      setError(null);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const handleAddTask = async (bucketId: string, title: string) => {
    await createTask({ title, bucket_id: bucketId });
    await reload();
  };

  const handleMoveTask = async (taskId: string, destBucketId: string, destIndex: number) => {
    if (!board) return;
    const destBucket = board.buckets.find(b => b.bucket.id === destBucketId);
    if (!destBucket) return;
    // Midpoint position between neighbours (Vikunja pattern)
    const tasks = destBucket.tasks;
    const before = tasks[destIndex - 1]?.position ?? 0;
    const after = tasks[destIndex]?.position ?? before + 2000;
    const position = (before + after) / 2;

    // Optimistic update
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

  const handleSaveTask = async (taskId: string, patch: Parameters<typeof updateTask>[0]['patch']) => {
    await updateTask({ task_id: taskId, patch });
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
      <div className="px-4 py-3 border-b border-stone-200 dark:border-neutral-800 shrink-0">
        <h1 className="text-base font-semibold text-stone-900 dark:text-neutral-100">
          {board.project.title}
        </h1>
      </div>

      <div className="flex-1 overflow-auto p-4">
        <KanbanBoard
          board={board}
          onTaskClick={setSelectedTask}
          onAddTask={handleAddTask}
          onMoveTask={handleMoveTask}
          onRenameColumn={handleRenameColumn}
        />
      </div>

      <TaskDetailDrawer
        task={selectedTask}
        onClose={() => setSelectedTask(null)}
        onSave={handleSaveTask}
        onDelete={handleDeleteTask}
      />
    </div>
  );
}
