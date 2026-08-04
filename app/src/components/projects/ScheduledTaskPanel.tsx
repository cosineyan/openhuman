import { useEffect, useState } from 'react';

import {
  type CoreCronJob,
  type CoreCronRun,
  openhumanCronAdd,
  openhumanCronList,
  openhumanCronRemove,
  openhumanCronRun,
  openhumanCronRuns,
  openhumanCronUpdate,
} from '../../utils/tauriCommands/cron';
import { ProfileModelPicker } from '../common/ProfileModelPicker';

// ─── helpers ────────────────────────────────────────────────────────────────

function fmtDate(iso: string | null | undefined) {
  if (!iso) return '—';
  try {
    return new Date(iso).toLocaleString(undefined, {
      month: 'short',
      day: 'numeric',
      hour: '2-digit',
      minute: '2-digit',
    });
  } catch {
    return iso;
  }
}

function scheduleLabel(job: CoreCronJob) {
  const s = job.schedule;
  if (s.kind === 'cron') return `Cron: ${s.expr}`;
  if (s.kind === 'every') {
    const ms = s.every_ms;
    if (ms < 60_000) return `Every ${ms / 1000}s`;
    if (ms < 3_600_000) return `Every ${ms / 60_000} min`;
    if (ms < 86_400_000) return `Every ${ms / 3_600_000}h`;
    return `Every ${ms / 86_400_000}d`;
  }
  if (s.kind === 'at') return `Once at ${fmtDate(s.at)}`;
  return '—';
}

// ─── Form state ─────────────────────────────────────────────────────────────

interface FormState {
  name: string;
  scheduleKind: 'cron' | 'every';
  cronExpr: string; // for kind=cron
  everyValue: string; // number
  everyUnit: 'minutes' | 'hours' | 'days';
  prompt: string;
  settingsProfile?: string;
  model?: string;
  fallbackDirection?: string;
  fallbackEnd?: string;
}

const EMPTY_FORM: FormState = {
  name: '',
  scheduleKind: 'cron',
  cronExpr: '0 9 * * 1-5',
  everyValue: '1',
  everyUnit: 'days',
  prompt: '',
};

function formToParams(f: FormState) {
  const schedule =
    f.scheduleKind === 'cron'
      ? { kind: 'cron' as const, expr: f.cronExpr.trim() }
      : (() => {
          const n = parseFloat(f.everyValue) || 1;
          const mul =
            f.everyUnit === 'minutes' ? 60_000 : f.everyUnit === 'hours' ? 3_600_000 : 86_400_000;
          return { kind: 'every' as const, every_ms: Math.round(n * mul) };
        })();
  return {
    name: f.name.trim() || undefined,
    schedule,
    job_type: 'agent' as const,
    prompt: f.prompt.trim(),
    session_target: 'isolated' as const,
    settings_profile: f.settingsProfile,
    model: f.model,
    fallback_direction: f.fallbackDirection,
    fallback_end: f.fallbackEnd,
    // This panel exists to create project-board tasks on a schedule, so the
    // fired job must deliver via "task" (which calls projects::create_task).
    // Without this the backend defaults delivery to "none" and nothing ever
    // lands on the board.
    delivery: { mode: 'task' as const, best_effort: true },
  };
}

// ─── Component ───────────────────────────────────────────────────────────────

export function ScheduledTaskPanel({ onOpenTask }: { onOpenTask?: (title: string) => void }) {
  const [tab, setTab] = useState<'tasks' | 'history'>('tasks');
  const [jobs, setJobs] = useState<CoreCronJob[]>([]);
  const [loading, setLoading] = useState(true);
  const [formOpen, setFormOpen] = useState(false);
  const [editingJobId, setEditingJobId] = useState<string | null>(null);
  const [form, setForm] = useState<FormState>(EMPTY_FORM);
  const [saving, setSaving] = useState(false);
  const [runningId, setRunningId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<Array<CoreCronRun & { job_name: string }>>([]);
  const [historyLoading, setHistoryLoading] = useState(false);

  const reload = async () => {
    try {
      const res = await openhumanCronList();
      // Only show agent jobs (not system shell jobs)
      setJobs((res.result ?? []).filter(j => j.job_type === 'agent'));
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to load');
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    void reload();
  }, []);

  const loadHistory = async (jobList: CoreCronJob[]) => {
    setHistoryLoading(true);
    try {
      const all: Array<CoreCronRun & { job_name: string }> = [];
      await Promise.all(
        jobList.map(async job => {
          try {
            const res = await openhumanCronRuns(job.id, 20);
            const runs = res.result ?? [];
            runs.forEach(r => all.push({ ...r, job_name: job.name ?? '(unnamed)' }));
          } catch {
            /* ignore per-job errors */
          }
        })
      );
      all.sort((a, b) => b.started_at.localeCompare(a.started_at));
      setHistory(all);
    } finally {
      setHistoryLoading(false);
    }
  };

  const handleTabChange = (t: 'tasks' | 'history') => {
    setTab(t);
    if (t === 'history') void loadHistory(jobs);
  };

  const handleSave = async () => {
    if (!form.prompt.trim()) {
      setError('Prompt is required');
      return;
    }
    setSaving(true);
    setError(null);
    try {
      if (editingJobId) {
        await openhumanCronUpdate(editingJobId, formToParams(form));
      } else {
        await openhumanCronAdd(formToParams(form));
      }
      setFormOpen(false);
      setEditingJobId(null);
      setForm(EMPTY_FORM);
      await reload();
    } catch (e: unknown) {
      setError(e instanceof Error ? e.message : 'Failed to save');
    } finally {
      setSaving(false);
    }
  };

  const handleEdit = (job: CoreCronJob) => {
    const s = job.schedule;
    let scheduleKind: FormState['scheduleKind'] = 'cron';
    let cronExpr = '0 9 * * 1-5';
    let everyValue = '1';
    let everyUnit: FormState['everyUnit'] = 'days';
    if (s.kind === 'cron') {
      scheduleKind = 'cron';
      cronExpr = s.expr;
    } else if (s.kind === 'every') {
      scheduleKind = 'every';
      const ms = s.every_ms;
      if (ms % 86_400_000 === 0) {
        everyValue = String(ms / 86_400_000);
        everyUnit = 'days';
      } else if (ms % 3_600_000 === 0) {
        everyValue = String(ms / 3_600_000);
        everyUnit = 'hours';
      } else {
        everyValue = String(ms / 60_000);
        everyUnit = 'minutes';
      }
    }
    setForm({
      name: job.name ?? '',
      scheduleKind,
      cronExpr,
      everyValue,
      everyUnit,
      prompt: job.prompt ?? '',
      settingsProfile: job.settings_profile ?? undefined,
      model: job.model ?? undefined,
      fallbackDirection: job.fallback_direction ?? undefined,
      fallbackEnd: job.fallback_end ?? undefined,
    });
    setEditingJobId(job.id);
    setFormOpen(true);
    setError(null);
  };

  const handleToggle = async (job: CoreCronJob) => {
    try {
      await openhumanCronUpdate(job.id, { enabled: !job.enabled });
      await reload();
    } catch {
      /* ignore */
    }
  };

  const handleDelete = async (id: string) => {
    if (!window.confirm('Delete this scheduled task?')) return;
    try {
      await openhumanCronRemove(id);
      await reload();
    } catch {
      /* ignore */
    }
  };

  const handleRunNow = async (id: string) => {
    setRunningId(id);
    try {
      await openhumanCronRun(id);
      await reload();
    } catch {
      /* ignore */
    } finally {
      setRunningId(null);
    }
  };

  const set = (patch: Partial<FormState>) => setForm(f => ({ ...f, ...patch }));

  return (
    <div style={{ padding: 24 }}>
      {/* Header */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 16,
        }}>
        <div>
          <h2 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>Scheduled Tasks</h2>
          <p style={{ margin: '4px 0 0', fontSize: 13, color: '#666' }}>
            Automatically create tasks on a schedule.
          </p>
        </div>
        <button
          onClick={() => {
            setFormOpen(true);
            setForm(EMPTY_FORM);
            setError(null);
          }}
          style={{
            padding: '6px 14px',
            borderRadius: 4,
            border: 'none',
            background: '#4A83DD',
            color: '#fff',
            cursor: 'pointer',
            fontSize: 13,
          }}>
          + Add Schedule
        </button>
      </div>

      {error && (
        <div
          style={{
            marginBottom: 12,
            padding: '8px 12px',
            borderRadius: 4,
            background: '#fff0f0',
            color: '#d32f2f',
            fontSize: 13,
          }}>
          {error}
        </div>
      )}

      {/* Tabs */}
      <div style={{ display: 'flex', borderBottom: '2px solid #f0f0f0', marginBottom: 16 }}>
        {(['tasks', 'history'] as const).map(t => (
          <button
            key={t}
            onClick={() => handleTabChange(t)}
            style={{
              padding: '6px 16px',
              fontSize: 13,
              border: 'none',
              background: 'none',
              cursor: 'pointer',
              fontWeight: tab === t ? 600 : 400,
              color: tab === t ? '#4A83DD' : '#888',
              borderBottom: `2px solid ${tab === t ? '#4A83DD' : 'transparent'}`,
              marginBottom: -2,
            }}>
            {t === 'tasks' ? 'Tasks' : 'History'}
          </button>
        ))}
      </div>

      {/* Create form */}
      {tab === 'tasks' && formOpen && (
        <div
          style={{
            marginBottom: 20,
            border: '1px solid #c5d8f5',
            borderRadius: 8,
            padding: 16,
            background: '#f8fbff',
          }}>
          <h3 style={{ margin: '0 0 12px', fontSize: 14, fontWeight: 600 }}>
            {editingJobId ? 'Edit Scheduled Task' : 'New Scheduled Task'}
          </h3>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            <label style={{ fontSize: 13 }}>
              <div style={{ marginBottom: 4, fontWeight: 500 }}>
                Name <span style={{ color: '#aaa', fontWeight: 400 }}>(optional)</span>
              </div>
              <input
                value={form.name}
                onChange={e => set({ name: e.target.value })}
                placeholder="e.g. Weekly review task"
                style={{
                  width: '100%',
                  padding: '6px 10px',
                  borderRadius: 4,
                  border: '1px solid #ddd',
                  fontSize: 13,
                  boxSizing: 'border-box',
                }}
              />
            </label>

            <label style={{ fontSize: 13 }}>
              <div style={{ marginBottom: 4, fontWeight: 500 }}>Schedule type</div>
              <div style={{ display: 'flex', gap: 8 }}>
                {(['cron', 'every'] as const).map(k => (
                  <button
                    key={k}
                    onClick={() => set({ scheduleKind: k })}
                    style={{
                      padding: '4px 12px',
                      borderRadius: 4,
                      border: `1px solid ${form.scheduleKind === k ? '#4A83DD' : '#ddd'}`,
                      background: form.scheduleKind === k ? '#EBF3FF' : '#fff',
                      color: form.scheduleKind === k ? '#1967d2' : '#444',
                      cursor: 'pointer',
                      fontSize: 12,
                    }}>
                    {k === 'cron' ? 'Cron expression' : 'Every N minutes/hours/days'}
                  </button>
                ))}
              </div>
            </label>

            {form.scheduleKind === 'cron' ? (
              <label style={{ fontSize: 13 }}>
                <div style={{ marginBottom: 4, fontWeight: 500 }}>Cron expression</div>
                <input
                  value={form.cronExpr}
                  onChange={e => set({ cronExpr: e.target.value })}
                  placeholder="0 9 * * 1-5"
                  style={{
                    width: '100%',
                    padding: '6px 10px',
                    borderRadius: 4,
                    border: '1px solid #ddd',
                    fontSize: 13,
                    fontFamily: 'monospace',
                    boxSizing: 'border-box',
                  }}
                />
                <div style={{ marginTop: 4, fontSize: 11, color: '#888' }}>
                  Format: minute hour day month weekday — e.g. <code>0 9 * * 1-5</code> = weekdays
                  at 9am
                </div>
              </label>
            ) : (
              <label style={{ fontSize: 13 }}>
                <div style={{ marginBottom: 4, fontWeight: 500 }}>Repeat every</div>
                <div style={{ display: 'flex', gap: 6, alignItems: 'center' }}>
                  <input
                    type="number"
                    min="1"
                    value={form.everyValue}
                    onChange={e => set({ everyValue: e.target.value })}
                    style={{
                      width: 70,
                      padding: '6px 10px',
                      borderRadius: 4,
                      border: '1px solid #ddd',
                      fontSize: 13,
                    }}
                  />
                  <select
                    value={form.everyUnit}
                    onChange={e => set({ everyUnit: e.target.value as FormState['everyUnit'] })}
                    style={{
                      padding: '6px 10px',
                      borderRadius: 4,
                      border: '1px solid #ddd',
                      fontSize: 13,
                    }}>
                    <option value="minutes">minutes</option>
                    <option value="hours">hours</option>
                    <option value="days">days</option>
                  </select>
                </div>
              </label>
            )}

            <label style={{ fontSize: 13 }}>
              <div style={{ marginBottom: 4, fontWeight: 500 }}>
                Task prompt <span style={{ color: '#d32f2f' }}>*</span>
              </div>
              <textarea
                value={form.prompt}
                onChange={e => set({ prompt: e.target.value })}
                placeholder="e.g. Create a task 'Weekly sprint review' in the projects board"
                rows={3}
                style={{
                  width: '100%',
                  padding: '6px 10px',
                  borderRadius: 4,
                  border: '1px solid #ddd',
                  fontSize: 13,
                  resize: 'vertical',
                  boxSizing: 'border-box',
                  fontFamily: 'inherit',
                }}
              />
            </label>
          </div>
          <div style={{ marginTop: 12 }}>
            <div style={{ marginBottom: 4, fontWeight: 500, fontSize: 13 }}>
              Claude profile & model
            </div>
            <ProfileModelPicker
              value={{
                settingsProfile: form.settingsProfile,
                model: form.model,
                fallbackDirection: form.fallbackDirection,
                fallbackEnd: form.fallbackEnd,
              }}
              onChange={v =>
                set({
                  settingsProfile: v.settingsProfile,
                  model: v.model,
                  fallbackDirection: v.fallbackDirection,
                  fallbackEnd: v.fallbackEnd,
                })
              }
            />
          </div>
          <div style={{ display: 'flex', gap: 8, marginTop: 14 }}>
            <button
              onClick={handleSave}
              disabled={saving}
              style={{
                padding: '6px 16px',
                borderRadius: 4,
                border: 'none',
                background: '#4A83DD',
                color: '#fff',
                cursor: saving ? 'not-allowed' : 'pointer',
                fontSize: 13,
              }}>
              {saving ? 'Saving…' : 'Save'}
            </button>
            <button
              onClick={() => {
                setFormOpen(false);
                setEditingJobId(null);
                setForm(EMPTY_FORM);
                setError(null);
              }}
              style={{
                padding: '6px 16px',
                borderRadius: 4,
                border: '1px solid #ddd',
                background: '#fff',
                cursor: 'pointer',
                fontSize: 13,
              }}>
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* History tab */}
      {tab === 'history' && (
        <div>
          {historyLoading ? (
            <div style={{ color: '#888', fontSize: 13 }}>Loading…</div>
          ) : history.length === 0 ? (
            <div style={{ color: '#888', fontSize: 13, padding: '24px 0', textAlign: 'center' }}>
              No runs yet.
            </div>
          ) : (
            <div style={{ overflowX: 'auto' }}>
              <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
                <thead>
                  <tr style={{ borderBottom: '1px solid #e5e7eb' }}>
                    {['Time', 'Task', 'Duration', 'Status', 'Project Task'].map(h => (
                      <th
                        key={h}
                        style={{
                          textAlign: 'left',
                          padding: '6px 10px',
                          fontSize: 11,
                          fontWeight: 600,
                          color: '#6b7280',
                          textTransform: 'uppercase',
                          letterSpacing: '0.05em',
                        }}>
                        {h}
                      </th>
                    ))}
                  </tr>
                </thead>
                <tbody>
                  {history.map(r => {
                    const runDate = r.started_at.slice(0, 10);
                    const taskTitle = `${r.job_name} — ${runDate}`;
                    return (
                      <tr key={r.id} style={{ borderBottom: '1px solid #f3f4f6' }}>
                        <td style={{ padding: '8px 10px', color: '#374151', whiteSpace: 'nowrap' }}>
                          {fmtDate(r.started_at)}
                        </td>
                        <td style={{ padding: '8px 10px' }}>
                          <span
                            style={{
                              fontSize: 11,
                              padding: '1px 6px',
                              background: '#dbeafe',
                              color: '#1d4ed8',
                              borderRadius: 4,
                            }}>
                            {r.job_name}
                          </span>
                        </td>
                        <td style={{ padding: '8px 10px', color: '#6b7280' }}>
                          {r.duration_ms ? `${(r.duration_ms / 1000).toFixed(1)}s` : '—'}
                        </td>
                        <td style={{ padding: '8px 10px' }}>
                          <span
                            style={{
                              color: r.status === 'ok' ? '#16a34a' : '#d32f2f',
                              fontWeight: 500,
                            }}>
                            {r.status}
                          </span>
                        </td>
                        <td style={{ padding: '8px 10px' }}>
                          {onOpenTask ? (
                            <button
                              onClick={() => onOpenTask(taskTitle)}
                              style={{
                                fontSize: 11,
                                color: '#4A83DD',
                                background: 'none',
                                border: 'none',
                                cursor: 'pointer',
                                padding: 0,
                                textDecoration: 'underline',
                              }}>
                              View task
                            </button>
                          ) : (
                            '—'
                          )}
                        </td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
              <div style={{ marginTop: 8, fontSize: 11, color: '#9ca3af' }}>
                {history.length} run(s)
              </div>
            </div>
          )}
        </div>
      )}

      {/* Job list */}
      {tab === 'tasks' &&
        (loading ? (
          <div style={{ color: '#888', fontSize: 13 }}>Loading…</div>
        ) : jobs.length === 0 ? (
          <div style={{ color: '#888', fontSize: 13, padding: '32px 0', textAlign: 'center' }}>
            No scheduled tasks yet. Add one to automatically create tasks on a schedule.
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
            {jobs.map(job => (
              <div
                key={job.id}
                style={{
                  border: '1px solid #e5e7eb',
                  borderRadius: 8,
                  padding: '12px 16px',
                  background: job.enabled ? '#fff' : '#fafafa',
                  opacity: job.enabled ? 1 : 0.65,
                }}>
                <div
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'space-between',
                  }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                    <input
                      type="checkbox"
                      checked={job.enabled}
                      onChange={() => handleToggle(job)}
                      style={{ cursor: 'pointer' }}
                    />
                    <span style={{ fontWeight: 600, fontSize: 14 }}>{job.name ?? '(unnamed)'}</span>
                    <span
                      style={{
                        fontSize: 11,
                        padding: '1px 7px',
                        background: '#f3f4f6',
                        color: '#6b7280',
                        borderRadius: 10,
                      }}>
                      {scheduleLabel(job)}
                    </span>
                  </div>
                  <div style={{ display: 'flex', gap: 6 }}>
                    <button
                      onClick={() => handleRunNow(job.id)}
                      disabled={runningId === job.id}
                      style={{
                        fontSize: 12,
                        padding: '3px 10px',
                        borderRadius: 4,
                        border: '1px solid #c5d8f5',
                        background: '#EBF3FF',
                        color: '#1967d2',
                        cursor: 'pointer',
                      }}>
                      {runningId === job.id ? 'Running…' : 'Run now'}
                    </button>
                    <button
                      onClick={() => handleEdit(job)}
                      style={{
                        fontSize: 12,
                        padding: '3px 10px',
                        borderRadius: 4,
                        border: '1px solid #ddd',
                        background: '#fff',
                        cursor: 'pointer',
                      }}>
                      Edit
                    </button>
                    <button
                      onClick={() => handleDelete(job.id)}
                      style={{
                        fontSize: 12,
                        padding: '3px 10px',
                        borderRadius: 4,
                        border: '1px solid #ddd',
                        background: '#fff',
                        color: '#d32f2f',
                        cursor: 'pointer',
                      }}>
                      Delete
                    </button>
                  </div>
                </div>
                {job.prompt && (
                  <div style={{ marginTop: 6, fontSize: 12, color: '#555', fontStyle: 'italic' }}>
                    "{job.prompt.length > 120 ? job.prompt.slice(0, 120) + '…' : job.prompt}"
                  </div>
                )}
                <div
                  style={{
                    marginTop: 6,
                    display: 'flex',
                    gap: 16,
                    fontSize: 11,
                    color: '#9ca3af',
                  }}>
                  <span>Next: {fmtDate(job.next_run)}</span>
                  {job.last_run && <span>Last: {fmtDate(job.last_run)}</span>}
                  {job.last_status && (
                    <span style={{ color: job.last_status === 'ok' ? '#16a34a' : '#d32f2f' }}>
                      {job.last_status}
                    </span>
                  )}
                </div>
              </div>
            ))}
          </div>
        ))}
    </div>
  );
}
