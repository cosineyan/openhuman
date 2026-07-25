import { useEffect, useState } from 'react';

import { listArchivedTasks, type Task } from '../../services/api/projectsApi';

interface Props {
  onTaskClick?: (task: Task) => void;
}

export function ArchivedView({ onTaskClick }: Props) {
  const [tasks, setTasks] = useState<Task[]>([]);
  const [loading, setLoading] = useState(true);
  const [search, setSearch] = useState('');
  const [createdAfter, setCreatedAfter] = useState('');
  const [createdBefore, setCreatedBefore] = useState('');

  const reload = async () => {
    setLoading(true);
    try {
      const result = await listArchivedTasks({
        search: search.trim() || undefined,
        // date inputs give "YYYY-MM-DD" — append time so RFC3339 parsing works on the backend
        created_after: createdAfter ? `${createdAfter}T00:00:00Z` : undefined,
        created_before: createdBefore ? `${createdBefore}T23:59:59Z` : undefined,
      });
      setTasks(result);
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  };

  // Initial load
  useEffect(() => { reload(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const fmt = (iso: string | null) => {
    if (!iso) return '—';
    try {
      return new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
    } catch {
      return iso;
    }
  };

  return (
    <div className="flex flex-col h-full p-4 overflow-auto">
      {/* Filters */}
      <div className="flex flex-wrap gap-3 mb-4 items-end">
        <div className="flex-1 min-w-48">
          <label className="block text-xs font-medium text-stone-500 dark:text-neutral-400 mb-1">
            Search title / description
          </label>
          <input
            type="text"
            value={search}
            onChange={e => setSearch(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && reload()}
            placeholder="Type and press Enter…"
            className="w-full px-3 py-1.5 text-sm border border-stone-200 dark:border-neutral-700 rounded-md bg-white dark:bg-neutral-900 text-stone-800 dark:text-neutral-100 placeholder-stone-400 focus:outline-none focus:ring-1 focus:ring-primary-500"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-stone-500 dark:text-neutral-400 mb-1">
            Created after
          </label>
          <input
            type="date"
            value={createdAfter}
            onChange={e => setCreatedAfter(e.target.value)}
            className="px-3 py-1.5 text-sm border border-stone-200 dark:border-neutral-700 rounded-md bg-white dark:bg-neutral-900 text-stone-800 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500"
          />
        </div>
        <div>
          <label className="block text-xs font-medium text-stone-500 dark:text-neutral-400 mb-1">
            Created before
          </label>
          <input
            type="date"
            value={createdBefore}
            onChange={e => setCreatedBefore(e.target.value)}
            className="px-3 py-1.5 text-sm border border-stone-200 dark:border-neutral-700 rounded-md bg-white dark:bg-neutral-900 text-stone-800 dark:text-neutral-100 focus:outline-none focus:ring-1 focus:ring-primary-500"
          />
        </div>
        <button
          onClick={reload}
          className="px-4 py-1.5 text-sm font-medium rounded-md bg-primary-500 text-white hover:bg-primary-600 transition-colors"
        >
          Filter
        </button>
      </div>

      {/* Table */}
      {loading ? (
        <div className="text-sm text-stone-400 dark:text-neutral-500">Loading…</div>
      ) : tasks.length === 0 ? (
        <div className="text-sm text-stone-400 dark:text-neutral-500 py-8 text-center">
          No archived tasks match the filter.
        </div>
      ) : (
        <div className="overflow-x-auto">
          <table className="w-full text-sm border-collapse">
            <thead>
              <tr className="border-b border-stone-200 dark:border-neutral-800">
                <th className="text-left py-2 px-3 text-xs font-semibold text-stone-500 dark:text-neutral-400 uppercase tracking-wide">
                  Title
                </th>
                <th className="text-left py-2 px-3 text-xs font-semibold text-stone-500 dark:text-neutral-400 uppercase tracking-wide">
                  Description
                </th>
                <th className="text-left py-2 px-3 text-xs font-semibold text-stone-500 dark:text-neutral-400 uppercase tracking-wide">
                  Created
                </th>
                <th className="text-left py-2 px-3 text-xs font-semibold text-stone-500 dark:text-neutral-400 uppercase tracking-wide">
                  Archived
                </th>
              </tr>
            </thead>
            <tbody>
              {tasks.map(task => (
                <tr
                  key={task.id}
                  onClick={() => onTaskClick?.(task)}
                  className={`border-b border-stone-100 dark:border-neutral-800/50 transition-colors ${
                    onTaskClick
                      ? 'cursor-pointer hover:bg-stone-50 dark:hover:bg-neutral-800/30'
                      : ''
                  }`}
                >
                  <td className="py-2 px-3 text-stone-700 dark:text-neutral-200 font-medium max-w-xs truncate">
                    {task.title}
                  </td>
                  <td className="py-2 px-3 text-stone-500 dark:text-neutral-400 max-w-xs truncate">
                    {task.description ?? '—'}
                  </td>
                  <td className="py-2 px-3 text-stone-500 dark:text-neutral-400 whitespace-nowrap">
                    {fmt(task.created)}
                  </td>
                  <td className="py-2 px-3 text-stone-500 dark:text-neutral-400 whitespace-nowrap">
                    {fmt(task.archived_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          <div className="mt-3 text-xs text-stone-400 dark:text-neutral-500">
            {tasks.length} archived task{tasks.length !== 1 ? 's' : ''}
          </div>
        </div>
      )}
    </div>
  );
}
