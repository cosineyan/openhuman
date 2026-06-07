import type { Task } from '../../services/api/projectsApi';

interface Props {
  task: Task;
  onClick: (task: Task) => void;
}

const PRIORITY_COLORS: Record<number, string> = {
  1: 'bg-sage-100 text-sage-800 dark:bg-sage-500/20 dark:text-sage-300',
  2: 'bg-primary-100 text-primary-800 dark:bg-primary-500/20 dark:text-primary-300',
  3: 'bg-amber-100 text-amber-800 dark:bg-amber-500/20 dark:text-amber-300',
  4: 'bg-coral-100 text-coral-800 dark:bg-coral-500/20 dark:text-coral-300',
  5: 'bg-coral-200 text-coral-900 dark:bg-coral-500/30 dark:text-coral-200',
};

const PRIORITY_LABELS: Record<number, string> = {
  1: 'Low', 2: 'Medium', 3: 'High', 4: 'Urgent', 5: 'Critical',
};

export function KanbanCard({ task, onClick }: Props) {
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onClick(task)}
      onKeyDown={e => e.key === 'Enter' && onClick(task)}
      className="rounded-lg border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 p-3 shadow-sm cursor-pointer hover:border-primary-300 dark:hover:border-primary-500/50 transition-colors"
    >
      <div className="flex items-start justify-between gap-2">
        <span className="text-xs text-stone-400 dark:text-neutral-500 shrink-0">
          #{task.index}
        </span>
        {task.priority > 0 && (
          <span className={`text-[10px] font-medium px-1.5 py-0.5 rounded ${PRIORITY_COLORS[task.priority] ?? ''}`}>
            {PRIORITY_LABELS[task.priority]}
          </span>
        )}
      </div>
      <p className={`mt-1 text-sm font-medium leading-snug ${task.done ? 'line-through text-stone-400 dark:text-neutral-500' : 'text-stone-900 dark:text-neutral-100'}`}>
        {task.title}
      </p>
      {task.due_date && (
        <p className="mt-1.5 text-xs text-stone-500 dark:text-neutral-400">
          Due {new Date(task.due_date).toLocaleDateString()}
        </p>
      )}
    </div>
  );
}
