import { useEffect, useRef } from 'react';

import { cancelAiTask, type Task } from '../../services/api/projectsApi';
import { type AiTaskRun } from './useAiTaskRuns';

interface Props {
  task: Task;
  run: AiTaskRun | undefined;
  onClose: () => void;
}

const STATUS_LABEL: Record<string, string> = {
  running: 'Running',
  done: 'Done',
  cancelled: 'Cancelled',
  error: 'Error',
};

const STATUS_COLOR: Record<string, string> = {
  running: 'bg-ocean-100 dark:bg-ocean-900 text-ocean-700 dark:text-ocean-300',
  done: 'bg-green-100 dark:bg-green-900 text-green-700 dark:text-green-300',
  cancelled: 'bg-stone-100 dark:bg-neutral-800 text-stone-500 dark:text-neutral-400',
  error: 'bg-red-100 dark:bg-red-900 text-red-700 dark:text-red-300',
};

export function AiRunDrawer({ task, run, onClose }: Props) {
  const logEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    logEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [run?.lines.length]);

  const status = run?.status ?? 'running';
  const lines = run?.lines ?? [];

  return (
    <>
      {/* Backdrop */}
      <div
        className="fixed inset-0 z-50 bg-black/30 backdrop-blur-sm"
        onClick={onClose}
      />

      {/* Drawer panel */}
      <div className="fixed top-0 right-0 bottom-0 z-50 w-[480px] max-w-full bg-white dark:bg-neutral-900 shadow-2xl flex flex-col">
        {/* Header */}
        <div className="flex items-start justify-between px-5 py-4 border-b border-stone-200 dark:border-neutral-800 shrink-0">
          <div className="flex-1 min-w-0 pr-3">
            <p className="text-xs text-stone-500 dark:text-neutral-400 mb-1">AI task run</p>
            <h3 className="text-sm font-semibold text-stone-900 dark:text-neutral-100 truncate">
              {task.title}
            </h3>
          </div>
          <div className="flex items-center gap-2 shrink-0">
            {/* Status chip */}
            <span
              className={`text-xs font-medium px-2 py-0.5 rounded-full flex items-center gap-1 ${STATUS_COLOR[status] ?? STATUS_COLOR.running}`}>
              {status === 'running' && (
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
              {STATUS_LABEL[status] ?? status}
            </span>
            {/* Stop button */}
            {status === 'running' && (
              <button
                onClick={() => void cancelAiTask(task.id)}
                className="text-xs font-medium px-2 py-0.5 rounded bg-red-50 dark:bg-red-950 text-red-600 dark:text-red-400 hover:bg-red-100 dark:hover:bg-red-900 border border-red-200 dark:border-red-800 transition-colors">
                Stop
              </button>
            )}
            {/* Close */}
            <button
              onClick={onClose}
              className="text-stone-400 dark:text-neutral-500 hover:text-stone-600 dark:hover:text-neutral-300 transition-colors p-1 rounded"
              aria-label="Close">
              <svg width="16" height="16" viewBox="0 0 16 16" fill="none">
                <path
                  d="M3 3l10 10M13 3L3 13"
                  stroke="currentColor"
                  strokeWidth="1.5"
                  strokeLinecap="round"
                />
              </svg>
            </button>
          </div>
        </div>

        {/* Log area */}
        <div className="flex-1 overflow-y-auto">
          <pre className="text-xs font-mono p-4 whitespace-pre-wrap break-words text-stone-700 dark:text-neutral-200 leading-relaxed min-h-full">
            {lines.length > 0 ? (
              lines.join('\n')
            ) : (
              <span className="text-stone-400 dark:text-neutral-500 italic">
                Waiting for activity…
              </span>
            )}
            <div ref={logEndRef} />
          </pre>
        </div>
      </div>
    </>
  );
}
