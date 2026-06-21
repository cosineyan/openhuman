import { useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { openhumanClaudeCodeResumeSession } from '../../utils/tauriCommands/config';

interface Props {
  sessionId: string;
  workspaceDir: string | null;
}

export function ClaudeCodeResumeCard({ sessionId, workspaceDir }: Props) {
  const { t } = useT();
  const command = workspaceDir
    ? `claude --resume ${sessionId} --add-dir "${workspaceDir}"`
    : `claude --resume ${sessionId}`;
  const [copyLabel, setCopyLabel] = useState<string | null>(null);
  const [openLabel, setOpenLabel] = useState<string | null>(null);
  const [openError, setOpenError] = useState<string | null>(null);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(command);
      setCopyLabel(t('projects.resumeCard.copied'));
      setTimeout(() => setCopyLabel(null), 1500);
    } catch {
      // clipboard unavailable — silent
    }
  };

  const handleOpen = async () => {
    setOpenError(null);
    setOpenLabel(t('projects.resumeCard.opening'));
    try {
      await openhumanClaudeCodeResumeSession(sessionId, workspaceDir ?? undefined);
      setOpenLabel(t('projects.resumeCard.opened'));
      setTimeout(() => setOpenLabel(null), 2000);
    } catch (_err) {
      setOpenLabel(null);
      setOpenError(t('projects.resumeCard.copyError'));
    }
  };

  return (
    <div className="mb-4 rounded-lg border border-stone-200 dark:border-neutral-700 overflow-hidden">
      {/* Header */}
      <div className="flex items-center gap-2 px-3 py-2 bg-stone-50 dark:bg-neutral-800 border-b border-stone-200 dark:border-neutral-700">
        {/* Terminal icon */}
        <svg
          width="14"
          height="14"
          viewBox="0 0 14 14"
          fill="none"
          className="text-stone-500 dark:text-neutral-400 shrink-0">
          <rect x="1" y="1" width="12" height="12" rx="2" stroke="currentColor" strokeWidth="1.2" />
          <path
            d="M3.5 5L5.5 7L3.5 9"
            stroke="currentColor"
            strokeWidth="1.2"
            strokeLinecap="round"
            strokeLinejoin="round"
          />
          <path d="M6.5 9H10" stroke="currentColor" strokeWidth="1.2" strokeLinecap="round" />
        </svg>
        <span className="text-xs font-medium text-stone-600 dark:text-neutral-300">
          {t('projects.resumeCard.title')}
        </span>
      </div>

      {/* Command row */}
      <div className="flex items-center gap-2 px-3 py-2 bg-white dark:bg-neutral-900">
        <code className="flex-1 text-xs font-mono text-stone-700 dark:text-neutral-300 truncate">
          {command}
        </code>
        <button
          type="button"
          onClick={() => void handleCopy()}
          className="shrink-0 text-xs text-stone-400 hover:text-stone-700 dark:text-neutral-500 dark:hover:text-neutral-200 transition-colors px-1.5 py-0.5 rounded">
          {copyLabel ?? (
            <svg width="13" height="13" viewBox="0 0 13 13" fill="none">
              <rect x="4.5" y="1" width="7.5" height="9" rx="1.5" stroke="currentColor" strokeWidth="1.2" />
              <path d="M1 4.5H8.5V12H1V4.5Z" stroke="currentColor" strokeWidth="1.2" />
            </svg>
          )}
        </button>
      </div>

      {/* Open button */}
      <div className="px-3 py-2 bg-stone-50 dark:bg-neutral-800 border-t border-stone-100 dark:border-neutral-800">
        <button
          type="button"
          onClick={() => void handleOpen()}
          disabled={openLabel !== null}
          className="w-full text-xs font-medium rounded-md bg-stone-900 dark:bg-neutral-100 text-white dark:text-neutral-900 py-1.5 hover:bg-stone-700 dark:hover:bg-neutral-300 disabled:opacity-60 transition-colors">
          {openLabel ?? t('projects.resumeCard.openTerminal')}
        </button>
        {openError && (
          <p className="mt-1.5 text-xs text-rose-600 dark:text-rose-400">{openError}</p>
        )}
      </div>
    </div>
  );
}
