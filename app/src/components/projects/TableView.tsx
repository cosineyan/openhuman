import type { BoardData, Bucket, Task } from '../../services/api/projectsApi';

interface Props {
  board: BoardData;
  onTaskClick: (task: Task) => void;
}

const PRIORITY_LABELS: Record<number, string> = {
  0: '—',
  1: 'Low',
  2: 'Medium',
  3: 'High',
  4: 'Urgent',
  5: 'Critical',
};
const PRIORITY_COLORS: Record<number, string> = {
  1: 'text-sage-600 dark:text-sage-400',
  2: 'text-primary-600 dark:text-primary-400',
  3: 'text-amber-600 dark:text-amber-400',
  4: 'text-coral-500 dark:text-coral-400',
  5: 'text-coral-600 dark:text-coral-300',
};

const COLS = [
  { key: 'index', label: '#', width: 'w-10', align: 'text-right' },
  { key: 'title', label: 'Title', width: 'flex-1', align: 'text-left' },
  { key: 'status', label: 'Status', width: 'w-28', align: 'text-left' },
  { key: 'assignee', label: 'Assignee', width: 'w-24', align: 'text-left' },
  { key: 'priority', label: 'Priority', width: 'w-24', align: 'text-left' },
  { key: 'due_date', label: 'Due date', width: 'w-28', align: 'text-left' },
] as const;

function bucketForTask(task: Task, board: BoardData): Bucket | undefined {
  return board.buckets.find(b => b.bucket.id === task.bucket_id)?.bucket;
}

export function TableView({ board, onTaskClick }: Props) {
  const allTasks = board.buckets.flatMap(b => b.tasks);
  allTasks.sort((a, b) => a.index - b.index);

  return (
    <div className="rounded-xl border border-stone-200 dark:border-neutral-800 overflow-hidden">
      {/* Header */}
      <div className="flex items-center gap-0 bg-stone-50 dark:bg-neutral-900 border-b border-stone-200 dark:border-neutral-800 px-4 py-2">
        {COLS.map(col => (
          <div
            key={col.key}
            className={`${col.width} ${col.align} text-xs font-semibold text-stone-500 dark:text-neutral-400 uppercase tracking-wide shrink-0 px-2 first:pl-0`}>
            {col.label}
          </div>
        ))}
      </div>

      {/* Rows */}
      {allTasks.length === 0 ? (
        <div className="px-4 py-6 text-sm text-stone-400 dark:text-neutral-500 text-center">
          No tasks yet
        </div>
      ) : (
        allTasks.map((task, i) => {
          const bucket = bucketForTask(task, board);
          return (
            <TableRow
              key={task.id}
              task={task}
              bucket={bucket}
              subtaskCount={(board.subtask_counts?.[task.id]?.[0]) ?? 0}
              isLast={i === allTasks.length - 1}
              onClick={onTaskClick}
            />
          );
        })
      )}
    </div>
  );
}

function TableRow({
  task,
  bucket,
  subtaskCount,
  isLast,
  onClick,
}: {
  task: Task;
  bucket: Bucket | undefined;
  subtaskCount: number;
  isLast: boolean;
  onClick: (task: Task) => void;
}) {
  return (
    <button
      type="button"
      onClick={() => onClick(task)}
      className={`w-full flex items-center gap-0 px-4 py-2.5 text-left bg-white dark:bg-neutral-900 hover:bg-stone-50 dark:hover:bg-neutral-800 transition-colors ${!isLast ? 'border-b border-stone-100 dark:border-neutral-800' : ''}`}>
      {/* # */}
      <div className="w-10 shrink-0 px-2 first:pl-0 text-right text-xs text-stone-400 dark:text-neutral-500">
        {task.index}
      </div>

      {/* Title */}
      <div className="flex-1 min-w-0 px-2 flex items-center gap-2">
        <span
          className={`shrink-0 w-3 h-3 rounded-full border flex items-center justify-center ${task.done ? 'bg-green-500 border-green-500' : 'border-stone-300 dark:border-neutral-600'}`}>
          {task.done && (
            <svg width="7" height="7" viewBox="0 0 8 8" fill="none">
              <path
                d="M1.5 4l2 2 3-3"
                stroke="white"
                strokeWidth="1.2"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          )}
        </span>
        <span
          className={`text-sm font-medium truncate ${task.done ? 'line-through text-stone-400 dark:text-neutral-500' : 'text-stone-800 dark:text-neutral-100'}`}>
          {task.title}
        </span>
        {subtaskCount > 0 && (
          <span className="shrink-0 flex items-center gap-0.5 text-[10px] text-stone-400 dark:text-neutral-500">
            <svg width="10" height="10" viewBox="0 0 12 12" fill="none">
              <path d="M2 2h8M2 6h5M2 10h3" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" />
            </svg>
            {subtaskCount}
          </span>
        )}
      </div>

      {/* Status */}
      <div className="w-28 shrink-0 px-2">
        {bucket && (
          <span className="text-xs text-stone-600 dark:text-neutral-300 bg-stone-100 dark:bg-neutral-800 px-1.5 py-0.5 rounded truncate block w-fit max-w-full">
            {bucket.title}
          </span>
        )}
      </div>

      {/* Assignee */}
      <div className="w-24 shrink-0 px-2">
        {task.assignee ? (
          <div className="flex items-center gap-1.5">
            <div className="w-5 h-5 rounded-full bg-stone-400 dark:bg-neutral-500 flex items-center justify-center shrink-0">
              <span className="text-[7px] font-bold text-white">
                {task.assignee === 'ai' ? 'AI' : 'ME'}
              </span>
            </div>
            <span className="text-xs text-stone-600 dark:text-neutral-300">
              {task.assignee === 'ai' ? 'AI' : 'Me'}
            </span>
          </div>
        ) : (
          <span className="text-xs text-stone-300 dark:text-neutral-600">—</span>
        )}
      </div>

      {/* Priority */}
      <div className="w-24 shrink-0 px-2">
        <span
          className={`text-xs font-medium ${task.priority > 0 ? (PRIORITY_COLORS[task.priority] ?? 'text-stone-400') : 'text-stone-300 dark:text-neutral-600'}`}>
          {PRIORITY_LABELS[task.priority] ?? '—'}
        </span>
      </div>

      {/* Due date */}
      <div className="w-28 shrink-0 px-2">
        {task.due_date ? (
          <span className="text-xs text-stone-500 dark:text-neutral-400">
            {(() => {
              const d = new Date(task.due_date!);
              const sameYear = d.getFullYear() === new Date().getFullYear();
              return d.toLocaleDateString(
                undefined,
                sameYear
                  ? { month: 'short', day: 'numeric' }
                  : { month: 'short', day: 'numeric', year: 'numeric' }
              );
            })()}
          </span>
        ) : (
          <span className="text-xs text-stone-300 dark:text-neutral-600">—</span>
        )}
      </div>
    </button>
  );
}
