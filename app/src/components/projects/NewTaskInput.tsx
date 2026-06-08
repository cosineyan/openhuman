import { useEffect, useRef, useState } from 'react';

interface Props {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  addTaskColor?: string;
  onAdd: (
    title: string,
    opts?: { assignee?: string; due_date?: string; priority?: number }
  ) => Promise<void>;
}

const PRIORITIES = [
  { value: 1, label: 'Low' },
  { value: 2, label: 'Medium' },
  { value: 3, label: 'High' },
  { value: 4, label: 'Urgent' },
  { value: 5, label: 'Critical' },
];

const QUICK_DATES: { label: string; days: number }[] = [
  { label: 'Today', days: 0 },
  { label: 'Tomorrow', days: 1 },
  { label: 'This weekend', days: -1 }, // computed below
  { label: 'Next week', days: 7 },
  { label: 'Next weekend', days: -2 },
  { label: '2 weeks', days: 14 },
  { label: '4 weeks', days: 28 },
];

function addDays(d: Date, n: number): Date {
  const r = new Date(d);
  r.setDate(r.getDate() + n);
  return r;
}

function nextWeekday(d: Date, dow: number): Date {
  const r = new Date(d);
  const diff = (dow - r.getDay() + 7) % 7 || 7;
  r.setDate(r.getDate() + diff);
  return r;
}

function toISO(d: Date): string {
  return d.toISOString().slice(0, 10);
}

function fmt(d: Date, opts?: Intl.DateTimeFormatOptions): string {
  return d.toLocaleDateString(undefined, opts ?? { weekday: 'short' });
}

function fmtShort(d: Date): string {
  return d.toLocaleDateString(undefined, { month: 'short', day: 'numeric' });
}

function PersonIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
      <circle cx="8" cy="5" r="3" stroke="currentColor" strokeWidth="1.5" />
      <path
        d="M2 14c0-3.314 2.686-6 6-6s6 2.686 6 6"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
      />
    </svg>
  );
}

function CalIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
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

function FlagIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
      <path
        d="M3 2v12M3 2h8l-2 3 2 3H3"
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ChevronIcon({ up }: { up?: boolean }) {
  return (
    <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
      <path
        d={up ? 'M2 8l4-4 4 4' : 'M2 4l4 4 4-4'}
        stroke="currentColor"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

// Simple inline calendar — rendered via a portal at fixed position
function CalendarPopover({
  value,
  onChange,
  onClose,
  anchor,
}: {
  value: string;
  onChange: (v: string) => void;
  onClose: () => void;
  anchor: { top: number; left: number };
}) {
  const today = new Date();
  today.setHours(0, 0, 0, 0);
  const [viewYear, setViewYear] = useState(today.getFullYear());
  const [viewMonth, setViewMonth] = useState(today.getMonth());
  const ref = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const handler = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) onClose();
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [onClose]);

  const firstDay = new Date(viewYear, viewMonth, 1);
  const lastDay = new Date(viewYear, viewMonth + 1, 0);
  const startDow = firstDay.getDay();
  const days: (Date | null)[] = [];
  for (let i = 0; i < startDow; i++) days.push(null);
  for (let d = 1; d <= lastDay.getDate(); d++) days.push(new Date(viewYear, viewMonth, d));

  const MONTH_NAMES = [
    'January',
    'February',
    'March',
    'April',
    'May',
    'June',
    'July',
    'August',
    'September',
    'October',
    'November',
    'December',
  ];
  const DOW = ['Su', 'Mo', 'Tu', 'We', 'Th', 'Fr', 'Sa'];

  // Quick shortcuts
  const sat = nextWeekday(today, 6);
  const nextSat = addDays(sat, 7);
  const shortcuts = [
    { label: 'Today', sub: fmt(today, { weekday: 'short' }), date: today },
    {
      label: 'Tomorrow',
      sub: fmt(addDays(today, 1), { weekday: 'short' }),
      date: addDays(today, 1),
    },
    { label: 'This weekend', sub: fmtShort(sat), date: sat },
    { label: 'Next week', sub: fmtShort(nextWeekday(today, 1)), date: nextWeekday(today, 1) },
    { label: 'Next weekend', sub: fmtShort(nextSat), date: nextSat },
    { label: '2 weeks', sub: fmtShort(addDays(today, 14)), date: addDays(today, 14) },
    { label: '4 weeks', sub: fmtShort(addDays(today, 28)), date: addDays(today, 28) },
  ];

  return (
    <div
      ref={ref}
      className="fixed z-[200] flex bg-white dark:bg-neutral-900 rounded-xl shadow-xl border border-stone-200 dark:border-neutral-700 overflow-hidden"
      style={{ top: anchor.top, left: anchor.left, minWidth: 480 }}>
      {/* Left: shortcuts */}
      <div className="w-48 border-r border-stone-100 dark:border-neutral-800 py-2">
        {shortcuts.map(s => (
          <button
            key={s.label}
            type="button"
            onClick={() => {
              onChange(toISO(s.date));
              onClose();
            }}
            className="w-full flex items-center justify-between px-4 py-1.5 text-sm text-stone-700 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800 transition-colors">
            <span>{s.label}</span>
            <span className="text-xs text-stone-400 dark:text-neutral-500">{s.sub}</span>
          </button>
        ))}
      </div>
      {/* Right: calendar */}
      <div className="p-3 select-none">
        {/* Month nav */}
        <div className="flex items-center justify-between mb-3 px-1">
          <span className="text-sm font-semibold text-stone-800 dark:text-neutral-200">
            {MONTH_NAMES[viewMonth]} {viewYear}
          </span>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={() => {
                const d = new Date(viewYear, viewMonth - 1);
                setViewYear(d.getFullYear());
                setViewMonth(d.getMonth());
              }}
              className="p-0.5 rounded hover:bg-stone-100 dark:hover:bg-neutral-800 text-stone-500">
              <ChevronIcon up />
            </button>
            <button
              type="button"
              onClick={() => setViewYear(viewYear)}
              className="text-xs text-primary-500 px-1.5 py-0.5 rounded hover:bg-stone-100 dark:hover:bg-neutral-800">
              Today
            </button>
            <button
              type="button"
              onClick={() => {
                const d = new Date(viewYear, viewMonth + 1);
                setViewYear(d.getFullYear());
                setViewMonth(d.getMonth());
              }}
              className="p-0.5 rounded hover:bg-stone-100 dark:hover:bg-neutral-800 text-stone-500">
              <ChevronIcon />
            </button>
          </div>
        </div>
        {/* DOW headers */}
        <div className="grid grid-cols-7 gap-1 mb-1">
          {DOW.map(d => (
            <span
              key={d}
              className="text-center text-[11px] text-stone-400 dark:text-neutral-500 font-medium w-8">
              {d}
            </span>
          ))}
        </div>
        {/* Days grid */}
        <div className="grid grid-cols-7 gap-1">
          {days.map((day, i) => {
            if (!day) return <span key={`e-${i}`} className="w-8 h-8" />;
            const iso = toISO(day);
            const isToday = iso === toISO(today);
            const isSelected = iso === value;
            const isPast = day < today;
            return (
              <button
                key={iso}
                type="button"
                onClick={() => {
                  onChange(iso);
                  onClose();
                }}
                className={`w-8 h-8 rounded-full text-sm font-medium transition-colors
                  ${isSelected ? 'bg-primary-500 text-white' : ''}
                  ${isToday && !isSelected ? 'bg-coral-500 text-white' : ''}
                  ${!isSelected && !isToday ? (isPast ? 'text-stone-300 dark:text-neutral-600 hover:bg-stone-50 dark:hover:bg-neutral-800' : 'text-stone-700 dark:text-neutral-300 hover:bg-stone-100 dark:hover:bg-neutral-800') : ''}
                `}>
                {day.getDate()}
              </button>
            );
          })}
        </div>
      </div>
    </div>
  );
}

export function NewTaskInput({ open: openProp, onOpenChange, addTaskColor, onAdd }: Props) {
  const [openInternal, setOpenInternal] = useState(false);
  const open = openProp ?? openInternal;

  const setOpen = (v: boolean) => {
    setOpenInternal(v);
    onOpenChange?.(v);
  };

  // Sync when parent forces open=true (e.g. clicking + in column header)
  useEffect(() => {
    if (openProp === true) setOpenInternal(true);
  }, [openProp]);
  const [title, setTitle] = useState('');
  const [assignee, setAssignee] = useState('');
  const [dueDate, setDueDate] = useState('');
  const [priority, setPriority] = useState(0);
  const [busy, setBusy] = useState(false);
  const [showDatePicker, setShowDatePicker] = useState(false);
  const [showAssigneePicker, setShowAssigneePicker] = useState(false);
  const [showPriorityPicker, setShowPriorityPicker] = useState(false);
  const [popoverAnchor, setPopoverAnchor] = useState({ top: 0, left: 0 });
  const containerRef = useRef<HTMLDivElement>(null);

  const openPopover = (
    e: React.MouseEvent<HTMLButtonElement>,
    setter: (v: boolean) => void,
    closeOthers: () => void
  ) => {
    closeOthers();
    const r = e.currentTarget.getBoundingClientRect();
    setPopoverAnchor({ top: r.bottom + 4, left: r.left });
    setter(v => !v);
  };

  // Close the whole form on outside click
  useEffect(() => {
    if (!open) return;
    const handler = (e: MouseEvent) => {
      if (containerRef.current && !containerRef.current.contains(e.target as Node)) {
        setOpen(false);
        setTitle('');
        setAssignee('');
        setDueDate('');
        setPriority(0);
        setShowDatePicker(false);
        setShowAssigneePicker(false);
        setShowPriorityPicker(false);
      }
    };
    document.addEventListener('mousedown', handler);
    return () => document.removeEventListener('mousedown', handler);
  }, [open]);

  const submit = async () => {
    const trimmed = title.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    try {
      await onAdd(trimmed, {
        assignee: assignee || undefined,
        due_date: dueDate || undefined,
        priority: priority || undefined,
      });
      setTitle('');
      setAssignee('');
      setDueDate('');
      setPriority(0);
      setOpen(false);
    } finally {
      setBusy(false);
    }
  };

  if (!open) {
    return (
      <button
        type="button"
        onClick={() => setOpen(true)}
        className={`flex items-center gap-1.5 w-full px-0.5 py-1.5 text-sm font-medium transition-colors ${addTaskColor ?? 'text-stone-400 dark:text-neutral-500 hover:text-stone-600 dark:hover:text-neutral-300'}`}>
        <span className="text-base leading-none font-light">+</span>
        <span>Add Task</span>
      </button>
    );
  }

  return (
    <div
      ref={containerRef}
      className="relative rounded-lg border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 shadow-sm">
      {/* Title + Save */}
      <div className="flex items-start justify-between gap-2 px-3 pt-3 pb-2">
        <input
          autoFocus
          type="text"
          value={title}
          onChange={e => setTitle(e.target.value)}
          onKeyDown={e => {
            if (e.key === 'Enter') void submit();
            if (e.key === 'Escape') {
              setOpen(false);
              setTitle('');
            }
          }}
          placeholder="Task Name..."
          className="flex-1 min-w-0 text-sm font-medium text-stone-900 dark:text-neutral-100 placeholder:text-stone-400 dark:placeholder:text-neutral-500 bg-transparent focus:outline-none"
        />
        <button
          type="button"
          disabled={!title.trim() || busy}
          onClick={() => void submit()}
          className="flex items-center gap-1 px-2.5 py-1 rounded-md bg-stone-200 dark:bg-neutral-700 text-xs font-medium text-stone-600 dark:text-neutral-300 hover:bg-stone-300 dark:hover:bg-neutral-600 disabled:opacity-30 transition-colors shrink-0">
          Save <kbd className="text-[10px] opacity-60">↵</kbd>
        </button>
      </div>

      {/* Hint rows */}
      <div className="border-t border-stone-100 dark:border-neutral-700">
        {/* Assignee row */}
        <div className="relative">
          <button
            type="button"
            onClick={e =>
              openPopover(e, setShowAssigneePicker, () => {
                setShowDatePicker(false);
                setShowPriorityPicker(false);
              })
            }
            className="flex items-center gap-2.5 w-full px-3 py-2 text-xs text-stone-500 dark:text-neutral-400 hover:bg-stone-50 dark:hover:bg-neutral-700/50 transition-colors">
            <PersonIcon />
            {assignee ? (
              <div className="flex items-center gap-1.5">
                <div className="w-4 h-4 rounded-full bg-stone-500 flex items-center justify-center">
                  <span className="text-[7px] font-bold text-white">
                    {assignee === 'ai' ? 'AI' : 'ME'}
                  </span>
                </div>
                <span>{assignee === 'ai' ? 'AI' : 'Me'}</span>
              </div>
            ) : (
              <span>Add assignee</span>
            )}
            {assignee && (
              <button
                type="button"
                onClick={e => {
                  e.stopPropagation();
                  setAssignee('');
                }}
                className="ml-auto text-stone-400 hover:text-stone-600 text-sm leading-none">
                ×
              </button>
            )}
          </button>
          {showAssigneePicker && (
            <div
              className="fixed z-[200] bg-white dark:bg-neutral-900 rounded-lg shadow-lg border border-stone-200 dark:border-neutral-700 py-1 w-36"
              style={{ top: popoverAnchor.top, left: popoverAnchor.left }}>
              {[
                { value: 'me', label: 'Me' },
                { value: 'ai', label: 'AI' },
              ].map(a => (
                <button
                  key={a.value}
                  type="button"
                  onClick={() => {
                    setAssignee(a.value);
                    setShowAssigneePicker(false);
                  }}
                  className="w-full text-left px-3 py-1.5 text-sm text-stone-700 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800">
                  {a.label}
                </button>
              ))}
            </div>
          )}
        </div>

        {/* Date row */}
        <div className="relative">
          <button
            type="button"
            onClick={e =>
              openPopover(e, setShowDatePicker, () => {
                setShowAssigneePicker(false);
                setShowPriorityPicker(false);
              })
            }
            className="flex items-center gap-2.5 w-full px-3 py-2 text-xs text-stone-500 dark:text-neutral-400 hover:bg-stone-50 dark:hover:bg-neutral-700/50 transition-colors">
            <CalIcon />
            {dueDate ? (
              <span className="text-stone-700 dark:text-neutral-300">
                {new Date(dueDate + 'T00:00:00').toLocaleDateString(undefined, {
                  month: 'short',
                  day: 'numeric',
                })}
              </span>
            ) : (
              <span>Add dates</span>
            )}
            {dueDate && (
              <button
                type="button"
                onClick={e => {
                  e.stopPropagation();
                  setDueDate('');
                }}
                className="ml-auto text-stone-400 hover:text-stone-600 text-sm leading-none">
                ×
              </button>
            )}
          </button>
          {showDatePicker && (
            <CalendarPopover
              value={dueDate}
              onChange={setDueDate}
              onClose={() => setShowDatePicker(false)}
              anchor={popoverAnchor}
            />
          )}
        </div>

        {/* Priority row */}
        <div className="relative">
          <button
            type="button"
            onClick={e =>
              openPopover(e, setShowPriorityPicker, () => {
                setShowAssigneePicker(false);
                setShowDatePicker(false);
              })
            }
            className="flex items-center gap-2.5 w-full px-3 py-2 text-xs text-stone-500 dark:text-neutral-400 hover:bg-stone-50 dark:hover:bg-neutral-700/50 transition-colors">
            <FlagIcon />
            {priority > 0 ? (
              <span className="text-stone-700 dark:text-neutral-300">
                {PRIORITIES.find(p => p.value === priority)?.label}
              </span>
            ) : (
              <span>Add priority</span>
            )}
            {priority > 0 && (
              <button
                type="button"
                onClick={e => {
                  e.stopPropagation();
                  setPriority(0);
                }}
                className="ml-auto text-stone-400 hover:text-stone-600 text-sm leading-none">
                ×
              </button>
            )}
          </button>
          {showPriorityPicker && (
            <div
              className="fixed z-[200] bg-white dark:bg-neutral-900 rounded-lg shadow-lg border border-stone-200 dark:border-neutral-700 py-1 w-36"
              style={{ top: popoverAnchor.top, left: popoverAnchor.left }}>
              {PRIORITIES.map(p => (
                <button
                  key={p.value}
                  type="button"
                  onClick={() => {
                    setPriority(p.value);
                    setShowPriorityPicker(false);
                  }}
                  className="w-full text-left px-3 py-1.5 text-sm text-stone-700 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800">
                  {p.label}
                </button>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
