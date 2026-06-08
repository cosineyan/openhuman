import type { Task } from '../../services/api/projectsApi';

interface Props {
  task: Task;
  onClick: (task: Task) => void;
}

const PRIORITY_FLAG_COLOR: Record<number, string> = {
  1: 'text-sage-400 dark:text-sage-500',
  2: 'text-primary-400 dark:text-primary-500',
  3: 'text-amber-400 dark:text-amber-500',
  4: 'text-coral-400 dark:text-coral-500',
  5: 'text-coral-500 dark:text-coral-400',
};

function FlagIcon({ className }: { className?: string }) {
  return (
    <svg className={className} width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path d="M3 2v12M3 2h8l-2 3 2 3H3" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round"/>
    </svg>
  );
}

function CalIcon() {
  return (
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <rect x="1.5" y="3" width="13" height="11" rx="1.5" stroke="currentColor" strokeWidth="1.5"/>
      <path d="M5 1.5V4M11 1.5V4M1.5 6.5h13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round"/>
    </svg>
  );
}

export function KanbanCard({ task, onClick }: Props) {
  const hasFooter = task.assignee || task.due_date || task.priority > 0;

  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onClick(task)}
      onKeyDown={e => e.key === 'Enter' && onClick(task)}
      className="rounded-lg bg-white dark:bg-neutral-800 border border-stone-200 dark:border-neutral-700 px-3 pt-3 pb-3 cursor-pointer hover:border-stone-300 dark:hover:border-neutral-600 hover:shadow-sm transition-all"
    >
      <p className={`text-sm font-medium leading-snug ${hasFooter ? 'mb-2' : ''} ${task.done ? 'line-through text-stone-400 dark:text-neutral-500' : 'text-stone-800 dark:text-neutral-100'}`}>
        {task.title}
      </p>

      {hasFooter && (
        <div className="flex items-center gap-2">
          {task.assignee && (
            <div className="w-5 h-5 rounded-full bg-stone-500 dark:bg-neutral-500 flex items-center justify-center shrink-0">
              <span className="text-[8px] font-bold text-white leading-none uppercase">
                {task.assignee === 'ai' ? 'AI' : 'ME'}
              </span>
            </div>
          )}
          {task.due_date && (
            <div className="flex items-center gap-1 text-stone-500 dark:text-neutral-400">
              <CalIcon />
              <span className="text-xs">
                {(() => {
                  const d = new Date(task.due_date);
                  const sameYear = d.getFullYear() === new Date().getFullYear();
                  return d.toLocaleDateString(undefined, sameYear
                    ? { month: 'short', day: 'numeric' }
                    : { month: 'short', day: 'numeric', year: 'numeric' });
                })()}
              </span>
            </div>
          )}
          {task.priority > 0 && (
            <FlagIcon className={`ml-auto ${PRIORITY_FLAG_COLOR[task.priority] ?? 'text-stone-400'}`} />
          )}
        </div>
      )}
    </div>
  );
}
