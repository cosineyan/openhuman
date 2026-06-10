import { useCallback, useEffect, useRef, useState } from 'react';

import { listRunningAiTasks } from '../../services/api/projectsApi';
import { socketService } from '../../services/socketService';

export type AiTaskRunStatus = 'running' | 'done' | 'cancelled' | 'error';

export interface AiTaskRun {
  taskId: string;
  lines: string[];
  status: AiTaskRunStatus;
}

type RunMap = Map<string, AiTaskRun>;

const TERMINAL_STATUSES: AiTaskRunStatus[] = ['done', 'cancelled', 'error'];
const CLEANUP_DELAY_MS = 30_000;

export function useAiTaskRuns() {
  const [runs, setRuns] = useState<RunMap>(new Map());
  const runsRef = useRef<RunMap>(runs);
  runsRef.current = runs;

  useEffect(() => {
    listRunningAiTasks()
      .then(({ task_ids }) => {
        if (task_ids.length === 0) return;
        setRuns(prev => {
          const next = new Map(prev);
          for (const id of task_ids) {
            if (!next.has(id)) {
              next.set(id, { taskId: id, lines: [], status: 'running' });
            }
          }
          return next;
        });
      })
      .catch(() => {
        // Non-fatal: run indicators won't show for pre-existing runs.
      });
  }, []);

  useEffect(() => {
    const listener = (data: unknown) => {
      if (!data || typeof data !== 'object') return;
      const raw = (data as Record<string, unknown>).output;
      if (typeof raw !== 'string') return;
      let parsed: { task_id?: string; line?: string; kind?: string };
      try {
        parsed = JSON.parse(raw) as typeof parsed;
      } catch {
        return;
      }
      const { task_id, line, kind } = parsed;
      if (!task_id || !line || !kind) return;

      const status: AiTaskRunStatus =
        kind === 'done' ? 'done'
        : kind === 'cancelled' ? 'cancelled'
        : kind === 'error' ? 'error'
        : 'running';

      setRuns(prev => {
        const next = new Map(prev);
        const existing = next.get(task_id);
        const run: AiTaskRun = {
          taskId: task_id,
          lines: existing ? [...existing.lines, line] : [line],
          status,
        };
        next.set(task_id, run);
        return next;
      });

      if (TERMINAL_STATUSES.includes(status)) {
        setTimeout(() => {
          setRuns(prev => {
            const next = new Map(prev);
            next.delete(task_id);
            return next;
          });
        }, CLEANUP_DELAY_MS);
      }
    };

    socketService.on('project:task_log', listener);
    return () => {
      socketService.off('project:task_log', listener);
    };
  }, []);

  const isRunning = useCallback(
    (taskId: string) => runs.get(taskId)?.status === 'running',
    [runs]
  );

  const getLines = useCallback(
    (taskId: string): string[] => runs.get(taskId)?.lines ?? [],
    [runs]
  );

  const getRun = useCallback(
    (taskId: string): AiTaskRun | undefined => runs.get(taskId),
    [runs]
  );

  return { isRunning, getLines, getRun, runs };
}
