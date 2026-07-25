import { useState } from 'react';

import type { CreateRuleInput, EmailAutomationRule, RulePatch } from '../../services/api/emailAutomationApi';

interface Props {
  rule?: EmailAutomationRule;
  onSave: (data: CreateRuleInput | RulePatch) => Promise<void>;
  onCancel: () => void;
}

export function EmailRuleForm({ rule, onSave, onCancel }: Props) {
  const [name, setName] = useState(rule?.name ?? '');
  const [enabled, setEnabled] = useState(rule?.enabled ?? true);
  const [senderContains, setSenderContains] = useState(rule?.sender_contains ?? '');
  const [subjectContains, setSubjectContains] = useState(rule?.subject_contains ?? '');
  const [bodyContains, setBodyContains] = useState(rule?.body_contains ?? '');
  const [taskTitle, setTaskTitle] = useState(rule?.task_title_template ?? '');
  const [taskDesc, setTaskDesc] = useState(rule?.task_description_template ?? '');
  const [llmFallback, setLlmFallback] = useState(rule?.llm_fallback_enabled ?? false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState('');

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) { setError('Rule name is required'); return; }
    if (!taskTitle.trim()) { setError('Task title template is required'); return; }

    setSaving(true);
    setError('');
    try {
      await onSave({
        name: name.trim(),
        enabled,
        sender_contains: senderContains.trim() || null,
        subject_contains: subjectContains.trim() || null,
        body_contains: bodyContains.trim() || null,
        task_title_template: taskTitle.trim(),
        task_description_template: taskDesc.trim() || null,
        assignee: 'ai',
        llm_fallback_enabled: llmFallback,
      });
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to save rule');
    } finally {
      setSaving(false);
    }
  };

  return (
    <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      {/* Name */}
      <div>
        <label style={{ display: 'block', fontSize: 12, fontWeight: 600, marginBottom: 4 }}>
          Rule name *
        </label>
        <input
          value={name}
          onChange={e => setName(e.target.value)}
          placeholder="e.g. Leave approval"
          style={{ width: '100%', padding: '6px 8px', borderRadius: 4, border: '1px solid #ccc', boxSizing: 'border-box' }}
        />
      </div>

      {/* Enabled */}
      <label style={{ display: 'flex', alignItems: 'center', gap: 8, cursor: 'pointer' }}>
        <input type="checkbox" checked={enabled} onChange={e => setEnabled(e.target.checked)} />
        <span style={{ fontSize: 13 }}>Enabled</span>
      </label>

      {/* Conditions */}
      <div style={{ background: '#f7f8fa', borderRadius: 6, padding: 12 }}>
        <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8, color: '#666' }}>
          Conditions (all must match)
        </div>
        {[
          { label: 'Sender contains', value: senderContains, onChange: setSenderContains, placeholder: 'e.g. hr@company.com' },
          { label: 'Subject contains', value: subjectContains, onChange: setSubjectContains, placeholder: 'e.g. Leave Request' },
          { label: 'Body contains', value: bodyContains, onChange: setBodyContains, placeholder: 'e.g. approval needed' },
        ].map(({ label, value, onChange, placeholder }) => (
          <div key={label} style={{ marginBottom: 8 }}>
            <label style={{ display: 'block', fontSize: 12, color: '#444', marginBottom: 3 }}>{label}</label>
            <input
              value={value}
              onChange={e => onChange(e.target.value)}
              placeholder={placeholder}
              style={{ width: '100%', padding: '5px 8px', borderRadius: 4, border: '1px solid #ddd', boxSizing: 'border-box', fontSize: 13 }}
            />
          </div>
        ))}
        <div style={{ fontSize: 11, color: '#999', marginTop: 4 }}>
          Leave blank to match any value for that field.
        </div>
      </div>

      {/* Action */}
      <div style={{ background: '#f7f8fa', borderRadius: 6, padding: 12 }}>
        <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8, color: '#666' }}>Action</div>
        <div style={{ marginBottom: 8 }}>
          <label style={{ display: 'block', fontSize: 12, color: '#444', marginBottom: 3 }}>
            Task title template *
          </label>
          <input
            value={taskTitle}
            onChange={e => setTaskTitle(e.target.value)}
            placeholder="e.g. Approve: {{subject}}"
            style={{ width: '100%', padding: '5px 8px', borderRadius: 4, border: '1px solid #ddd', boxSizing: 'border-box', fontSize: 13 }}
          />
          <div style={{ fontSize: 11, color: '#999', marginTop: 3 }}>
            Use {'{{subject}}'}, {'{{sender}}'}, {'{{body_preview}}'} as placeholders.
          </div>
        </div>
        <div>
          <label style={{ display: 'block', fontSize: 12, color: '#444', marginBottom: 3 }}>
            Task description (optional)
          </label>
          <textarea
            value={taskDesc}
            onChange={e => setTaskDesc(e.target.value)}
            placeholder="Optional description for the created task…"
            rows={2}
            style={{ width: '100%', padding: '5px 8px', borderRadius: 4, border: '1px solid #ddd', boxSizing: 'border-box', fontSize: 13, resize: 'vertical' }}
          />
        </div>
      </div>

      {/* LLM fallback */}
      <label style={{ display: 'flex', alignItems: 'flex-start', gap: 8, cursor: 'pointer' }}>
        <input
          type="checkbox"
          checked={llmFallback}
          onChange={e => setLlmFallback(e.target.checked)}
          style={{ marginTop: 2 }}
        />
        <div>
          <div style={{ fontSize: 13 }}>Use AI when no rule matches</div>
          <div style={{ fontSize: 11, color: '#888', marginTop: 2 }}>
            When no condition matches, the AI decides whether this email needs a task.
          </div>
        </div>
      </label>

      {error && (
        <div style={{ fontSize: 12, color: '#d32f2f', padding: '6px 8px', background: '#fff0f0', borderRadius: 4 }}>
          {error}
        </div>
      )}

      <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 4 }}>
        <button
          type="button"
          onClick={onCancel}
          style={{ padding: '6px 14px', borderRadius: 4, border: '1px solid #ccc', background: '#fff', cursor: 'pointer', fontSize: 13 }}
        >
          Cancel
        </button>
        <button
          type="submit"
          disabled={saving}
          style={{ padding: '6px 14px', borderRadius: 4, border: 'none', background: '#4A83DD', color: '#fff', cursor: saving ? 'not-allowed' : 'pointer', fontSize: 13 }}
        >
          {saving ? 'Saving…' : 'Save rule'}
        </button>
      </div>
    </form>
  );
}
