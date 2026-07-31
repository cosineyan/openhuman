import { useCallback, useEffect, useState } from 'react';

import {
  createRule,
  type CreateRuleInput,
  deleteRule,
  type EmailAutomationRule,
  type EmailContentResult,
  getEmailContent,
  listProcessedEmails,
  listRules,
  type ProcessedEmailEntry,
  type RulePatch,
  runNow,
  type RunNowResult,
  updateRule,
} from '../../services/api/emailAutomationApi';
import { EmailRuleForm } from './EmailRuleForm';

interface Props {
  onOpenTask?: (taskId: string) => void;
}

export function EmailAutomationPanel({ onOpenTask }: Props) {
  const [tab, setTab] = useState<'rules' | 'history'>('rules');
  const [rules, setRules] = useState<EmailAutomationRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [scanResult, setScanResult] = useState<RunNowResult | null>(null);
  const [scanError, setScanError] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editingRule, setEditingRule] = useState<EmailAutomationRule | null>(null);
  const [history, setHistory] = useState<ProcessedEmailEntry[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [emailModal, setEmailModal] = useState<EmailContentResult | null>(null);
  const [emailModalLoading, setEmailModalLoading] = useState(false);

  const reload = useCallback(async () => {
    try {
      setRules(await listRules());
    } catch {
      /* ignore */
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    reload();
  }, [reload]);

  const handleSave = async (data: CreateRuleInput | RulePatch) => {
    if (editingRule) await updateRule(editingRule.id, data as RulePatch);
    else await createRule(data as CreateRuleInput);
    setFormOpen(false);
    setEditingRule(null);
    reload();
  };

  const handleDelete = async (id: string) => {
    if (!confirm('Delete this rule?')) return;
    await deleteRule(id);
    reload();
  };

  const handleToggle = async (rule: EmailAutomationRule) => {
    await updateRule(rule.id, { enabled: !rule.enabled });
    reload();
  };

  const handleScanNow = async (hours?: number) => {
    setScanning(true);
    setScanResult(null);
    setScanError(null);
    try {
      const result = await runNow(50, hours);
      setScanResult(result);
    } catch (err: unknown) {
      setScanError(err instanceof Error ? err.message : 'unknown error');
    } finally {
      setScanning(false);
    }
  };

  const handleTabChange = (t: 'rules' | 'history') => {
    setTab(t);
    if (t === 'history') {
      setHistoryLoading(true);
      listProcessedEmails(100)
        .then(setHistory)
        .catch(() => {})
        .finally(() => setHistoryLoading(false));
    }
  };

  const handleShowEmail = async (sourceId: string) => {
    setEmailModalLoading(true);
    setEmailModal(null);
    try {
      const result = await getEmailContent(sourceId);
      setEmailModal(
        result ?? {
          subject: '(not found)',
          from: '',
          to: '',
          date: '',
          body: 'Email content not available.',
        }
      );
    } catch {
      setEmailModal({
        subject: 'Error',
        from: '',
        to: '',
        date: '',
        body: 'Failed to load email.',
      });
    } finally {
      setEmailModalLoading(false);
    }
  };

  const handleOpenTask = (taskId: string) => {
    if (onOpenTask) {
      onOpenTask(taskId);
    }
  };

  const conditionSummary = (rule: EmailAutomationRule) => {
    const parts: string[] = [];
    if (rule.sender_contains) parts.push(`from: ${rule.sender_contains}`);
    if (rule.subject_contains) parts.push(`subject: ${rule.subject_contains}`);
    if (rule.body_contains) parts.push(`body: ${rule.body_contains}`);
    return parts.length > 0 ? parts.join(' · ') : 'Match all emails';
  };

  const fmt = (iso: string) => {
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
  };

  if (formOpen) {
    return (
      <div
        style={{
          padding: 24,
          width: '100%',
          height: '100%',
          overflowY: 'auto',
          boxSizing: 'border-box',
        }}>
        <h3 style={{ margin: '0 0 16px', fontSize: 15 }}>
          {editingRule ? 'Edit rule' : 'New rule'}
        </h3>
        <EmailRuleForm
          rule={editingRule ?? undefined}
          onSave={handleSave}
          onCancel={() => {
            setFormOpen(false);
            setEditingRule(null);
          }}
        />
      </div>
    );
  }

  return (
    <div style={{ padding: 24 }}>
      {/* Email modal */}
      {(emailModal || emailModalLoading) && (
        <div
          style={{
            position: 'fixed',
            inset: 0,
            zIndex: 50,
            background: 'rgba(0,0,0,0.4)',
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
          }}
          onClick={() => setEmailModal(null)}>
          <div
            style={{
              background: '#fff',
              borderRadius: 12,
              padding: 24,
              width: 600,
              maxWidth: '95vw',
              maxHeight: '80vh',
              display: 'flex',
              flexDirection: 'column',
              gap: 12,
              boxShadow: '0 8px 32px rgba(0,0,0,0.18)',
            }}
            onClick={e => e.stopPropagation()}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
              <h3 style={{ margin: 0, fontSize: 14, fontWeight: 700 }}>
                {emailModal?.subject ?? 'Loading…'}
              </h3>
              <button
                onClick={() => setEmailModal(null)}
                style={{
                  border: 'none',
                  background: 'none',
                  fontSize: 20,
                  cursor: 'pointer',
                  color: '#888',
                }}>
                ×
              </button>
            </div>
            {emailModalLoading ? (
              <div style={{ color: '#888', fontSize: 13 }}>Loading email…</div>
            ) : emailModal ? (
              <>
                <div
                  style={{
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 4,
                    fontSize: 13,
                    color: '#374151',
                    borderBottom: '1px solid #e5e7eb',
                    paddingBottom: 10,
                  }}>
                  {emailModal.from && (
                    <div>
                      <span
                        style={{
                          fontWeight: 600,
                          color: '#6b7280',
                          display: 'inline-block',
                          minWidth: 52,
                        }}>
                        From:
                      </span>{' '}
                      {emailModal.from}
                    </div>
                  )}
                  {emailModal.to && (
                    <div>
                      <span
                        style={{
                          fontWeight: 600,
                          color: '#6b7280',
                          display: 'inline-block',
                          minWidth: 52,
                        }}>
                        To:
                      </span>{' '}
                      {emailModal.to}
                    </div>
                  )}
                  {emailModal.date && (
                    <div>
                      <span
                        style={{
                          fontWeight: 600,
                          color: '#6b7280',
                          display: 'inline-block',
                          minWidth: 52,
                        }}>
                        Date:
                      </span>{' '}
                      {emailModal.date}
                    </div>
                  )}
                </div>
                <div
                  style={{
                    flex: 1,
                    overflowY: 'auto',
                    fontSize: 13,
                    whiteSpace: 'pre-wrap',
                    fontFamily: 'inherit',
                    background: '#f9fafb',
                    borderRadius: 6,
                    padding: 12,
                    maxHeight: 380,
                  }}>
                  {emailModal.body}
                </div>
              </>
            ) : null}
          </div>
        </div>
      )}

      {/* Header */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          marginBottom: 16,
        }}>
        <div>
          <h2 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>Email Automation</h2>
          <p style={{ margin: '4px 0 0', fontSize: 13, color: '#666' }}>
            Automatically create tasks when emails match your rules.
          </p>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            onClick={() => handleScanNow()}
            disabled={scanning}
            style={{
              padding: '6px 14px',
              borderRadius: 4,
              border: '1px solid #ccc',
              background: '#fff',
              cursor: scanning ? 'not-allowed' : 'pointer',
              fontSize: 13,
            }}>
            {scanning ? 'Scanning…' : 'Scan now'}
          </button>
          <button
            onClick={() => handleScanNow(24)}
            disabled={scanning}
            style={{
              padding: '6px 14px',
              borderRadius: 4,
              border: '1px solid #4A83DD',
              background: '#EBF3FF',
              color: '#1967d2',
              cursor: scanning ? 'not-allowed' : 'pointer',
              fontSize: 13,
            }}>
            {scanning ? 'Scanning…' : 'Force scan 24h'}
          </button>
          <button
            onClick={() => {
              setEditingRule(null);
              setFormOpen(true);
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
            + Add Rule
          </button>
        </div>
      </div>

      {scanError && (
        <div
          style={{
            marginBottom: 16,
            padding: '8px 12px',
            borderRadius: 4,
            background: '#fff0f0',
            fontSize: 13,
            color: '#d32f2f',
          }}>
          Error: {scanError}
        </div>
      )}

      {scanResult && (
        <div
          style={{
            marginBottom: 16,
            borderRadius: 6,
            border: '1px solid #c5d8f5',
            background: '#f0f7ff',
            fontSize: 13,
          }}>
          <div
            style={{
              padding: '8px 12px',
              color: '#1a5fa8',
              fontWeight: 600,
              borderBottom: scanResult.hits.length > 0 ? '1px solid #c5d8f5' : 'none',
            }}>
            {scanResult.tasks_created} task(s) created from {scanResult.emails_scanned} emails
            scanned
          </div>
          {scanResult.hits.length > 0 && (
            <div style={{ padding: '8px 12px', display: 'flex', flexDirection: 'column', gap: 4 }}>
              {scanResult.hits.map((hit, i) => (
                <div key={i} style={{ display: 'flex', gap: 8, alignItems: 'flex-start' }}>
                  <span
                    style={{
                      fontSize: 11,
                      padding: '1px 6px',
                      background: '#dbeafe',
                      color: '#1d4ed8',
                      borderRadius: 4,
                      flexShrink: 0,
                    }}>
                    {hit.rule_name}
                  </span>
                  <span style={{ fontSize: 12, color: '#374151' }}>{hit.task_title}</span>
                </div>
              ))}
            </div>
          )}
          {scanResult.tasks_created === 0 && (
            <div style={{ padding: '4px 12px 8px', fontSize: 12, color: '#6b7280' }}>
              No rules matched.
            </div>
          )}
        </div>
      )}

      {/* Tabs */}
      <div style={{ display: 'flex', borderBottom: '2px solid #f0f0f0', marginBottom: 16 }}>
        {(['rules', 'history'] as const).map(t => (
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
            {t === 'rules' ? 'Rules' : 'History'}
          </button>
        ))}
      </div>

      {/* History tab */}
      {tab === 'history' && (
        <div>
          {historyLoading ? (
            <div style={{ fontSize: 13, color: '#888' }}>Loading…</div>
          ) : history.length === 0 ? (
            <div style={{ fontSize: 13, color: '#888', padding: '24px 0', textAlign: 'center' }}>
              No emails have been processed yet.
            </div>
          ) : (
            <div style={{ overflowX: 'auto' }}>
              <table style={{ width: '100%', fontSize: 12, borderCollapse: 'collapse' }}>
                <thead>
                  <tr style={{ borderBottom: '1px solid #e5e7eb' }}>
                    {['Time', 'Rule', 'Email', 'Task'].map(h => (
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
                  {history.map((e, i) => (
                    <tr key={i} style={{ borderBottom: '1px solid #f3f4f6' }}>
                      <td style={{ padding: '8px 10px', color: '#374151', whiteSpace: 'nowrap' }}>
                        {fmt(e.processed_at)}
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
                          {e.rule_name}
                        </span>
                      </td>
                      <td style={{ padding: '8px 10px' }}>
                        <button
                          onClick={() => handleShowEmail(e.source_id)}
                          style={{
                            fontSize: 11,
                            color: '#4A83DD',
                            background: 'none',
                            border: 'none',
                            cursor: 'pointer',
                            padding: 0,
                            textDecoration: 'underline',
                          }}>
                          View email
                        </button>
                      </td>
                      <td style={{ padding: '8px 10px' }}>
                        <button
                          onClick={() => handleOpenTask(e.task_id)}
                          style={{
                            fontSize: 11,
                            color: '#4A83DD',
                            background: 'none',
                            border: 'none',
                            cursor: 'pointer',
                            padding: 0,
                            textDecoration: 'underline',
                            fontFamily: 'monospace',
                          }}>
                          {e.task_id.slice(0, 8)}…
                        </button>
                      </td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div style={{ marginTop: 8, fontSize: 11, color: '#9ca3af' }}>
                {history.length} record(s)
              </div>
            </div>
          )}
        </div>
      )}

      {/* Rules tab */}
      {tab === 'rules' && (
        <div>
          {loading ? (
            <div style={{ color: '#888', fontSize: 13 }}>Loading rules…</div>
          ) : rules.length === 0 ? (
            <div style={{ color: '#888', fontSize: 13, padding: '32px 0', textAlign: 'center' }}>
              No rules yet. Add a rule to automatically create tasks from emails.
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
              {rules.map(rule => (
                <div
                  key={rule.id}
                  style={{
                    border: '1px solid #e5e7eb',
                    borderRadius: 8,
                    padding: '12px 16px',
                    background: rule.enabled ? '#fff' : '#fafafa',
                    opacity: rule.enabled ? 1 : 0.65,
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
                        checked={rule.enabled}
                        onChange={() => handleToggle(rule)}
                        style={{ cursor: 'pointer' }}
                      />
                      <span style={{ fontWeight: 600, fontSize: 14 }}>{rule.name}</span>
                    </div>
                    <div style={{ display: 'flex', gap: 6 }}>
                      <button
                        onClick={() => {
                          setEditingRule(rule);
                          setFormOpen(true);
                        }}
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
                        onClick={() => handleDelete(rule.id)}
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
                  <div style={{ marginTop: 6, fontSize: 12, color: '#666' }}>
                    <span style={{ marginRight: 12 }}>When: {conditionSummary(rule)}</span>
                  </div>
                  <div style={{ marginTop: 4, fontSize: 12, color: '#444' }}>
                    → Create task: <em>{rule.task_title_template}</em>
                    {rule.assignee === 'ai' && (
                      <span style={{ marginLeft: 8, color: '#888' }}>assigned to AI</span>
                    )}
                    {rule.parse_script && (
                      <span
                        style={{
                          marginLeft: 8,
                          fontSize: 11,
                          padding: '1px 5px',
                          background: '#e8f5e9',
                          color: '#2e7d32',
                          borderRadius: 8,
                        }}>
                        ⚙ parse script
                      </span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
