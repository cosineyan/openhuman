import type { BoardData, Bucket, Task } from '../../services/api/projectsApi';

interface Props {
  board: BoardData;
  onTaskClick: (task: Task) => void;
}

const PRIORITY_LABELS: Record<number, string> = {
  1: 'Low', 2: 'Medium', 3: 'High', 4: 'Urgent', 5: 'Critical',
};
const PRIORITY_COLORS: Record<number, string> = {
  1: 'text-sage-600 dark:text-sage-400',
  2: 'text-primary-600 dark:text-primary-400',
  3: 'text-amber-600 dark:text-amber-400',
  4: 'text-coral-500 dark:text-coral-400',
  5: 'text-coral-600 dark:text-coral-300',
};

function StatusDot({ bucket }: { bucket: Bucket }) {
  const t = bucket.title.toLowerCase();
  if (bucket.is_done_bucket || t.includes('done') || t.includes('complete')) {
    return <span className="inline-block w-2 h-2 rounded-full bg-green-500 shrink-0" />;
  }
  if (t.includes('progress') || t.includes('doing')) {
    return <span className="inline-block w-2 h-2 rounded-full bg-primary-500 shrink-0" />;
  }
  if (t.includes('block')) {
    return <span className="inline-block w-2 h-2 rounded-full bg-coral-400 shrink-0" />;
  }
  return <span className="inline-block w-2 h-2 rounded-full border border-stone-400 dark:border-neutral-500 shrink-0" />;
}

export function ListView({ board, onTaskClick }: Props) {
  return (
    <div className="space-y-6">
      {board.buckets.map(({ bucket, tasks }) => (
        <div key={bucket.id}>
          {/* Group header */}
          <div className="flex items-center gap-2 mb-2 px-1">
            <StatusDot bucket={bucket} />
            <span className="text-xs font-bold tracking-widest uppercase text-stone-600 dark:text-neutral-400">
              {bucket.title}
            </span>
            <span className="text-xs text-stone-400 dark:text-neutral-500 ml-1">{tasks.length}</span>
          </div>

          {/* Task rows */}
          <div className="rounded-xl border border-stone-200 dark:border-neutral-800 overflow-hidden">
            {tasks.length === 0 ? (
              <div className="px-4 py-3 text-xs text-stone-400 dark:text-neutral-500 italic">
                No tasks
              </div>
            ) : (
              tasks.map((task, i) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  bucket={bucket}
                  isLast={i === tasks.length - 1}
                  onClick={onTaskClick}
                />
              ))
            )}
          </div>
        </div>
      ))}
    </div>
  );
}

function TaskRow({ task, bucket, isLast, onClick }: {
  task: Task;
  bucket: Bucket;
  isLast: boolean;
  onClick: (task: Task) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onClick(task)}
      className={`w-full flex items-center gap-3 px-4 py-2.5 text-left bg-white dark:bg-neutral-900 hover:bg-stone-50 dark:hover:bg-neutral-800 transition-colors ${!isLast ? 'border-b border-stone-100 dark:border-neutral-800' : ''}`}
    >
      {/* Done checkbox-style dot */}
      <span className={`shrink-0 w-3.5 h-3.5 rounded-full border flex items-center justify-center ${task.done ? 'bg-green-500 border-green-500' : 'border-stone-300 dark:border-neutral-600'}`}>
        {task.done && (
          <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
            <path d="M1.5 4l2 2 3-3" stroke="white" strokeWidth="1.2" strokeLinecap="round" strokeLinejoin="round"/>
          </svg>
        )}
      </span>

      {/* Title */}
      <span className={`flex-1 text-sm font-medium min-w-0 truncate ${task.done ? 'line-through text-stone-400 dark:text-neutral-500' : 'text-stone-800 dark:text-neutral-100'}`}>
        {task.title}
      </span>

      {/* Assignee */}
      {task.assignee && (
        <div className="shrink-0 w-5 h-5 rounded-full bg-stone-400 dark:bg-neutral-500 flex items-center justify-center">
          <span className="text-[7px] font-bold text-white">{task.assignee === 'ai' ? 'AI' : 'ME'}</span>
        </div>
      )}

      {/* Due date */}
      {task.due_date && (
        <span className="shrink-0 text-xs text-stone-400 dark:text-neutral-500">
          {(() => {
            const d = new Date(task.due_date!);
            const sameYear = d.getFullYear() === new Date().getFullYear();
            return d.toLocaleDateString(undefined, sameYear
              ? { month: 'short', day: 'numeric' }
              : { month: 'short', day: 'numeric', year: 'numeric' });
          })()}
        </span>
      )}

      {/* Priority */}
      {task.priority > 0 && (
        <span className={`shrink-0 text-xs font-medium ${PRIORITY_COLORS[task.priority] ?? 'text-stone-400'}`}>
          {PRIORITY_LABELS[task.priority]}
        </span>
      )}

      {/* Status pill */}
      <span className="shrink-0 text-[10px] text-stone-400 dark:text-neutral-500 bg-stone-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded">
        {bucket.title}
      </span>
    </button>
  );
}
