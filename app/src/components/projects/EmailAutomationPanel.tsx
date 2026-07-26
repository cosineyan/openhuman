import { useCallback, useEffect, useState } from 'react';

import {
  createRule,
  deleteRule,
  listRules,
  runNow,
  updateRule,
  type CreateRuleInput,
  type EmailAutomationRule,
  type RulePatch,
} from '../../services/api/emailAutomationApi';
import { EmailRuleForm } from './EmailRuleForm';

export function EmailAutomationPanel() {
  const [rules, setRules] = useState<EmailAutomationRule[]>([]);
  const [loading, setLoading] = useState(true);
  const [scanning, setScanning] = useState(false);
  const [scanResult, setScanResult] = useState<string | null>(null);
  const [formOpen, setFormOpen] = useState(false);
  const [editingRule, setEditingRule] = useState<EmailAutomationRule | null>(null);

  const reload = useCallback(async () => {
    try {
      setRules(await listRules());
    } catch {
      // ignore
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => { reload(); }, [reload]);

  const handleSave = async (data: CreateRuleInput | RulePatch) => {
    if (editingRule) {
      await updateRule(editingRule.id, data as RulePatch);
    } else {
      await createRule(data as CreateRuleInput);
    }
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

  const handleScanNow = async () => {
    setScanning(true);
    setScanResult(null);
    try {
      const result = await runNow(50);
      setScanResult(`${result.tasks_created} task(s) created from ${result.emails_scanned} emails scanned`);
    } catch (err: unknown) {
      setScanResult(`Error: ${err instanceof Error ? err.message : 'unknown error'}`);
    } finally {
      setScanning(false);
    }
  };

  const conditionSummary = (rule: EmailAutomationRule) => {
    const parts: string[] = [];
    if (rule.sender_contains) parts.push(`from: ${rule.sender_contains}`);
    if (rule.subject_contains) parts.push(`subject: ${rule.subject_contains}`);
    if (rule.body_contains) parts.push(`body: ${rule.body_contains}`);
    return parts.length > 0 ? parts.join(' · ') : 'Match all emails';
  };

  if (formOpen) {
    return (
      <div style={{ padding: 24, maxWidth: 520 }}>
        <h3 style={{ margin: '0 0 16px', fontSize: 15 }}>
          {editingRule ? 'Edit rule' : 'New rule'}
        </h3>
        <EmailRuleForm
          rule={editingRule ?? undefined}
          onSave={handleSave}
          onCancel={() => { setFormOpen(false); setEditingRule(null); }}
        />
      </div>
    );
  }

  return (
    <div style={{ padding: 24 }}>
      {/* Header */}
      <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 20 }}>
        <div>
          <h2 style={{ margin: 0, fontSize: 16, fontWeight: 700 }}>Email Automation</h2>
          <p style={{ margin: '4px 0 0', fontSize: 13, color: '#666' }}>
            Automatically create tasks when emails match your rules.
          </p>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          <button
            onClick={handleScanNow}
            disabled={scanning}
            style={{ padding: '6px 14px', borderRadius: 4, border: '1px solid #ccc', background: '#fff', cursor: scanning ? 'not-allowed' : 'pointer', fontSize: 13 }}
          >
            {scanning ? 'Scanning…' : 'Scan now'}
          </button>
          <button
            onClick={() => { setEditingRule(null); setFormOpen(true); }}
            style={{ padding: '6px 14px', borderRadius: 4, border: 'none', background: '#4A83DD', color: '#fff', cursor: 'pointer', fontSize: 13 }}
          >
            + Add Rule
          </button>
        </div>
      </div>

      {scanResult && (
        <div style={{ marginBottom: 16, padding: '8px 12px', borderRadius: 4, background: '#f0f7ff', fontSize: 13, color: '#1a5fa8' }}>
          {scanResult}
        </div>
      )}

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
              }}
            >
              <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <input
                    type="checkbox"
                    checked={rule.enabled}
                    onChange={() => handleToggle(rule)}
                    style={{ cursor: 'pointer' }}
                  />
                  <span style={{ fontWeight: 600, fontSize: 14 }}>{rule.name}</span>
                  {rule.llm_fallback_enabled && (
                    <span style={{ fontSize: 11, padding: '2px 6px', background: '#e8f0fe', color: '#1967d2', borderRadius: 10 }}>
                      AI fallback
                    </span>
                  )}
                </div>
                <div style={{ display: 'flex', gap: 6 }}>
                  <button
                    onClick={() => { setEditingRule(rule); setFormOpen(true); }}
                    style={{ fontSize: 12, padding: '3px 10px', borderRadius: 4, border: '1px solid #ddd', background: '#fff', cursor: 'pointer' }}
                  >
                    Edit
                  </button>
                  <button
                    onClick={() => handleDelete(rule.id)}
                    style={{ fontSize: 12, padding: '3px 10px', borderRadius: 4, border: '1px solid #ddd', background: '#fff', color: '#d32f2f', cursor: 'pointer' }}
                  >
                    Delete
                  </button>
                </div>
              </div>
              <div style={{ marginTop: 6, fontSize: 12, color: '#666' }}>
                <span style={{ marginRight: 12 }}>When: {conditionSummary(rule)}</span>
              </div>
              <div style={{ marginTop: 4, fontSize: 12, color: '#444' }}>
                → Create task: <em>{rule.task_title_template}</em>
                {rule.assignee === 'ai' && <span style={{ marginLeft: 8, color: '#888' }}>assigned to AI</span>}
                {rule.parse_script && (
                  <span style={{ marginLeft: 8, fontSize: 11, padding: '1px 5px', background: '#e8f5e9', color: '#2e7d32', borderRadius: 8 }}>
                    ⚙ parse script
                  </span>
                )}
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
