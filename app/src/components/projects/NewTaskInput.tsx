import { useState } from 'react';

interface Props {
  onAdd: (title: string) => Promise<void>;
}

export function NewTaskInput({ onAdd }: Props) {
  const [value, setValue] = useState('');
  const [busy, setBusy] = useState(false);

  const submit = async () => {
    const trimmed = value.trim();
    if (!trimmed || busy) return;
    setBusy(true);
    try {
      await onAdd(trimmed);
      setValue('');
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="pt-2">
      <input
        type="text"
        value={value}
        onChange={e => setValue(e.target.value)}
        onKeyDown={e => e.key === 'Enter' && void submit()}
        placeholder="Add a task…"
        disabled={busy}
        className="w-full rounded-lg border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 px-3 py-2 text-sm text-stone-900 dark:text-neutral-100 placeholder:text-stone-400 dark:placeholder:text-neutral-500 focus:outline-none focus:ring-1 focus:ring-primary-500 disabled:opacity-50"
      />
    </div>
  );
}
