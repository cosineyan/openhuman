import { useEffect, useState } from 'react';

import { listSubtasks, type Task } from '../../services/api/projectsApi';
import { AiRunDrawer } from './AiRunDrawer';
import { useAiTaskRuns } from './useAiTaskRuns';

interface Props {
  task: Task;
  /** [total, done] subtask counts */
  subtaskInfo?: [number, number];
  /** Increments on every board reload — triggers subtask refresh when expanded */
  boardVersion?: number;
  onClick: (task: Task) => void;
}

const PRIORITY_FLAG_COLOR: Record<number, string> = {
  1: 'text-stone-400 dark:text-neutral-500',
  2: 'text-primary-400 dark:text-primary-500',
  3: 'text-amber-400 dark:text-amber-500',
  4: 'text-coral-400 dark:text-coral-500',
  5: 'text-coral-500 dark:text-coral-400',
};

function FlagIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="11" height="11" viewBox="0 0 16 16" fill="none">
      <path
        d="M3 2v12M3 2h8l-2 3 2 3H3"
        stroke="currentColor"
        strokeWidth="1.6"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function CalIcon() {
  return (
    <svg width="11" height="11" viewBox="0 0 16 16" fill="none">
      <rect x="1.5" y="3" width="13" height="11" rx="1.5" stroke="currentColor" strokeWidth="1.5" />
      <path
        d="M5 1.5V4M11 1.5V4M1.5 6.5h13"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function SubMiniCard({ sub, onClick }: { sub: Task; onClick: (t: Task) => void }) {
  const hasFooter = sub.assignee || sub.due_date || sub.priority > 0;
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onClick(sub)}
      onKeyDown={e => e.key === 'Enter' && onClick(sub)}
      className="rounded-lg bg-white dark:bg-neutral-800 border border-stone-200 dark:border-neutral-700 px-3 pt-2.5 pb-2.5 cursor-pointer hover:border-stone-300 dark:hover:border-neutral-600 hover:shadow-sm transition-all">
      <p
        className={`text-sm font-medium leading-snug ${hasFooter ? 'mb-2' : ''} ${sub.done ? 'line-through text-stone-400 dark:text-neutral-500' : 'text-stone-800 dark:text-neutral-100'}`}>
        {sub.title}
      </p>
      {hasFooter && (
        <div className="flex items-center gap-2">
          {sub.assignee && (
            <div className="w-5 h-5 rounded-full bg-stone-500 dark:bg-neutral-500 flex items-center justify-center shrink-0">
              <span className="text-[8px] font-bold text-white leading-none uppercase">
                {sub.assignee === 'ai' ? 'AI' : 'ME'}
              </span>
            </div>
          )}
          {sub.due_date && (
            <div className="flex items-center gap-1 text-stone-500 dark:text-neutral-400 border border-stone-200 dark:border-neutral-700 rounded px-1.5 py-0.5">
              <CalIcon />
              <span className="text-xs">
                {(() => {
                  const d = new Date(sub.due_date!);
                  const sameYear = d.getFullYear() === new Date().getFullYear();
                  return d.toLocaleDateString(
                    undefined,
                    sameYear
                      ? { month: 'short', day: 'numeric' }
                      : { month: 'short', day: 'numeric', year: 'numeric' }
                  );
                })()}
              </span>
            </div>
          )}
          {sub.priority > 0 && (
            <div
              className={`flex items-center border border-stone-200 dark:border-neutral-700 rounded px-1.5 py-0.5 ${PRIORITY_FLAG_COLOR[sub.priority]}`}>
              <FlagIcon />
            </div>
          )}
        </div>
      )}
    </div>
  );
}

export function KanbanCard({ task, subtaskInfo, boardVersion, onClick }: Props) {
  const [expanded, setExpanded] = useState(false);
  const [subtasks, setSubtasks] = useState<Task[]>([]);
  const [total, done] = subtaskInfo ?? [0, 0];
  const hasSubtasks = total > 0;
  const hasFooter = task.assignee || task.due_date || task.priority > 0;

  const { isRunning, getLines } = useAiTaskRuns();
  const aiRunning = task.assignee === 'ai' && isRunning(task.id);
  const lastLogLine = aiRunning ? getLines(task.id).at(-1) : undefined;
  const [showRunDrawer, setShowRunDrawer] = useState(false);

  // Re-fetch subtasks whenever the board reloads (boardVersion changes) and we're expanded
  useEffect(() => {
    if (expanded && hasSubtasks) {
      void listSubtasks(task.id).then(setSubtasks);
    }
  }, [boardVersion, expanded, hasSubtasks, task.id]);

  const toggleExpand = async (e: React.MouseEvent) => {
    e.stopPropagation();
    const next = !expanded;
    setExpanded(next);
    if (next) {
      const subs = await listSubtasks(task.id);
      setSubtasks(subs);
    }
  };

  return (
    <div className="flex flex-col gap-1.5">
      {/* Main card */}
      <div
        className="rounded-lg bg-white dark:bg-neutral-800 border border-stone-200 dark:border-neutral-700 px-3 pt-3 pb-3 cursor-pointer hover:border-stone-300 dark:hover:border-neutral-600 hover:shadow-sm transition-all"
        role="button"
        tabIndex={0}
        onClick={() => onClick(task)}
        onKeyDown={e => e.key === 'Enter' && onClick(task)}>
        <p
          className={`text-sm font-medium leading-snug ${hasFooter || hasSubtasks ? 'mb-2' : ''} ${task.done ? 'line-through text-stone-400 dark:text-neutral-500' : 'text-stone-800 dark:text-neutral-100'}`}>
          {task.title}
        </p>
        {lastLogLine && (
          <button
            onClick={e => {
              e.stopPropagation();
              setShowRunDrawer(true);
            }}
            className="w-full text-left text-xs text-ocean-600 dark:text-ocean-400 truncate mt-0.5 hover:underline">
            {lastLogLine}
          </button>
        )}

        {/* Subtask count row — separate from assignee/date/priority */}
        {hasSubtasks && (
          <div className="flex items-center gap-1.5 mb-1.5">
            {/* done/total progress circle icon */}
            <svg
              width="13"
              height="13"
              viewBox="0 0 14 14"
              fill="none"
              className="text-stone-400 dark:text-neutral-500 shrink-0">
              <circle cx="7" cy="7" r="5.5" stroke="currentColor" strokeWidth="1.3" />
              <path
                d="M4.5 7l2 2 3-3"
                stroke="currentColor"
                strokeWidth="1.3"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
            <span className="text-xs text-stone-500 dark:text-neutral-400">
              {done}/{total}
            </span>
          </div>
        )}

        {hasFooter && (
          <div className="flex items-center gap-2">
            {task.assignee && (
              <span className="text-xs font-medium px-1.5 py-0.5 rounded bg-ocean-100 dark:bg-ocean-900 text-ocean-700 dark:text-ocean-300 flex items-center gap-1">
                {aiRunning && (
                  <svg className="animate-spin h-3 w-3" viewBox="0 0 24 24" fill="none">
                    <circle
                      className="opacity-25"
                      cx="12"
                      cy="12"
                      r="10"
                      stroke="currentColor"
                      strokeWidth="4"
                    />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8v8H4z" />
                  </svg>
                )}
                {task.assignee === 'ai' ? 'AI' : 'ME'}
              </span>
            )}
            {task.due_date && (
              <div className="flex items-center gap-1 text-stone-500 dark:text-neutral-400 border border-stone-200 dark:border-neutral-700 rounded px-1.5 py-0.5">
                <CalIcon />
                <span className="text-xs">
                  {(() => {
                    const d = new Date(task.due_date);
                    const sameYear = d.getFullYear() === new Date().getFullYear();
                    return d.toLocaleDateString(
                      undefined,
                      sameYear
                        ? { month: 'short', day: 'numeric' }
                        : { month: 'short', day: 'numeric', year: 'numeric' }
                    );
                  })()}
                </span>
              </div>
            )}
            {task.priority > 0 && (
              <div
                className={`flex items-center border border-stone-200 dark:border-neutral-700 rounded px-1.5 py-0.5 ${PRIORITY_FLAG_COLOR[task.priority]}`}>
                <FlagIcon />
              </div>
            )}
          </div>
        )}
      </div>

      {/* Expand toggle row */}
      {hasSubtasks && (
        <button
          type="button"
          onClick={toggleExpand}
          className="flex items-center gap-1.5 px-1 py-0.5 text-xs text-stone-500 dark:text-neutral-400 hover:text-stone-700 dark:hover:text-neutral-200 transition-colors">
          <svg
            width="8"
            height="8"
            viewBox="0 0 8 8"
            fill="currentColor"
            className={`transition-transform duration-150 ${expanded ? '' : '-rotate-90'}`}>
            <path d="M4 5.5L1 2.5h6L4 5.5z" />
          </svg>
          <span>
            {total} subtask{total !== 1 ? 's' : ''}
          </span>
        </button>
      )}

      {/* Expanded subtask mini-cards */}
      {expanded && subtasks.length > 0 && (
        <div className="flex flex-col gap-1.5 pl-3 border-l-2 border-stone-200 dark:border-neutral-700">
          {subtasks.map(sub => (
            <SubMiniCard key={sub.id} sub={sub} onClick={onClick} />
          ))}
        </div>
      )}
      {showRunDrawer && (
        <AiRunDrawer task={task} onClose={() => setShowRunDrawer(false)} />
      )}
    </div>
  );
}
