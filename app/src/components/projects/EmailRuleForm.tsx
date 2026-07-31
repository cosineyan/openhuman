import { useState } from 'react';

import {
  type BatchParseMode,
  type CreateRuleInput,
  type DryRunResult,
  dryRunRule,
  type EmailAutomationRule,
  refineRule,
  type RulePatch,
  searchEmailChunks,
} from '../../services/api/emailAutomationApi';
import { EmailPickerModal } from './EmailPickerModal';

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
  const [parseScript, setParseScript] = useState(rule?.parse_script ?? '');
  const [showScript, setShowScript] = useState(!!rule?.parse_script);
  const [batchMode, setBatchMode] = useState(rule?.batch_mode ?? false);
  const [batchWindowHours, setBatchWindowHours] = useState(
    Math.round((rule?.batch_window_secs ?? 21600) / 3600)
  );
  const [batchParseMode, setBatchParseMode] = useState<BatchParseMode>(
    rule?.batch_parse_mode ?? 'first_only'
  );
  const [saving, setSaving] = useState(false);
  const [showPicker, setShowPicker] = useState(false);
  const generating = false;
  const [error, setError] = useState('');

  // Dry run state
  const [showDryRun, setShowDryRun] = useState(false);
  const [dryRunMode, setDryRunMode] = useState<'manual' | 'pick'>('manual');
  const [dryRunBody, setDryRunBody] = useState('');
  const [dryRunChunkId, setDryRunChunkId] = useState<string | undefined>(undefined);
  const [dryRunRunning, setDryRunRunning] = useState(false);
  const [dryRunResult, setDryRunResult] = useState<DryRunResult | null>(null);
  const [dryRunError, setDryRunError] = useState('');
  const [refineFeedback, setRefineFeedback] = useState('');
  const [refining, setRefining] = useState(false);

  const handleDryRun = async () => {
    if (!dryRunBody.trim()) {
      setDryRunError('Paste an email body first');
      return;
    }
    setDryRunRunning(true);
    setDryRunError('');
    setDryRunResult(null);
    try {
      const result = await dryRunRule({
        task_title_template: taskTitle,
        task_description_template: taskDesc || null,
        parse_script: parseScript || null,
        email_body: dryRunBody,
      });
      setDryRunResult(result);
    } catch (err: unknown) {
      setDryRunError(err instanceof Error ? err.message : 'Dry run failed');
    } finally {
      setDryRunRunning(false);
    }
  };

  const handleRefine = async () => {
    const body = dryRunBody.trim();
    // Allow refine if we have a chunk_id (full body via RPC) or manual body text
    if ((!body && !dryRunChunkId) || !refineFeedback.trim()) return;
    setRefining(true);
    setDryRunError('');
    try {
      const refined = await refineRule({
        task_title_template: taskTitle,
        task_description_template: taskDesc || null,
        parse_script: parseScript || null,
        email_body: body || undefined,
        chunk_id: dryRunChunkId,
        user_feedback: refineFeedback,
      });
      // Apply refined values back to form
      if (refined.name) setName(refined.name);
      if (refined.sender_contains != null) setSenderContains(refined.sender_contains ?? '');
      if (refined.subject_contains != null) setSubjectContains(refined.subject_contains ?? '');
      if (refined.task_title_template) setTaskTitle(refined.task_title_template);
      if (refined.task_description_template != null)
        setTaskDesc(refined.task_description_template ?? '');
      if (refined.parse_script != null) {
        setParseScript(refined.parse_script ?? '');
        if (refined.parse_script) setShowScript(true);
      }
      setRefineFeedback('');
      // Auto re-run dry run with updated values — use refined values directly (state updates async)
      setDryRunRunning(true);
      setDryRunResult(null);
      setDryRunMode('manual');
      setShowDryRun(true);
      try {
        const result = await dryRunRule({
          task_title_template: refined.task_title_template || taskTitle,
          task_description_template: refined.task_description_template || taskDesc || null,
          parse_script: refined.parse_script || parseScript || null,
          email_body: body,
        });
        setDryRunResult(result);
      } finally {
        setDryRunRunning(false);
      }
    } catch (err: unknown) {
      setDryRunError(err instanceof Error ? err.message : 'Refinement failed');
    } finally {
      setRefining(false);
    }
  };

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) {
      setError('Rule name is required');
      return;
    }
    if (!taskTitle.trim()) {
      setError('Task title template is required');
      return;
    }

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
        parse_script: parseScript.trim() || null,
        batch_mode: batchMode,
        batch_window_secs: batchWindowHours * 3600,
        batch_parse_mode: batchParseMode,
      });
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to save rule');
    } finally {
      setSaving(false);
    }
  };

  const handleEmailSelected = async (
    suggestion: import('../../services/api/emailAutomationApi').CreateRuleInput
  ) => {
    setShowPicker(false);
    if (suggestion.name) setName(suggestion.name);
    if (suggestion.sender_contains) setSenderContains(suggestion.sender_contains);
    if (suggestion.subject_contains) setSubjectContains(suggestion.subject_contains);
    if (suggestion.task_title_template) setTaskTitle(suggestion.task_title_template);
    if (suggestion.task_description_template) setTaskDesc(suggestion.task_description_template);
    if (suggestion.parse_script) {
      setParseScript(suggestion.parse_script);
      setShowScript(true);
    }
  };

  return (
    <>
      {showPicker && (
        <EmailPickerModal onGenerate={handleEmailSelected} onCancel={() => setShowPicker(false)} />
      )}
      <form onSubmit={handleSubmit} style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        {/* Generate from email button */}
        <div style={{ display: 'flex', justifyContent: 'flex-end' }}>
          <button
            type="button"
            onClick={() => setShowPicker(true)}
            disabled={generating}
            style={{
              fontSize: 12,
              padding: '4px 12px',
              borderRadius: 6,
              border: '1px solid #4A83DD',
              color: '#4A83DD',
              background: '#EBF3FF',
              cursor: generating ? 'not-allowed' : 'pointer',
            }}>
            {generating ? 'Generating…' : '✦ Generate from email'}
          </button>
        </div>
        {/* Name */}
        <div>
          <label style={{ display: 'block', fontSize: 12, fontWeight: 600, marginBottom: 4 }}>
            Rule name *
          </label>
          <input
            value={name}
            onChange={e => setName(e.target.value)}
            placeholder="e.g. Leave approval"
            style={{
              width: '100%',
              padding: '6px 8px',
              borderRadius: 4,
              border: '1px solid #ccc',
              boxSizing: 'border-box',
            }}
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
            {
              label: 'Sender contains',
              value: senderContains,
              onChange: setSenderContains,
              placeholder: 'e.g. hr@company.com',
            },
            {
              label: 'Subject contains',
              value: subjectContains,
              onChange: setSubjectContains,
              placeholder: 'e.g. Leave Request',
            },
            {
              label: 'Body contains',
              value: bodyContains,
              onChange: setBodyContains,
              placeholder: 'e.g. approval needed',
            },
          ].map(({ label, value, onChange, placeholder }) => (
            <div key={label} style={{ marginBottom: 8 }}>
              <label style={{ display: 'block', fontSize: 12, color: '#444', marginBottom: 3 }}>
                {label}
              </label>
              <input
                value={value}
                onChange={e => onChange(e.target.value)}
                placeholder={placeholder}
                style={{
                  width: '100%',
                  padding: '5px 8px',
                  borderRadius: 4,
                  border: '1px solid #ddd',
                  boxSizing: 'border-box',
                  fontSize: 13,
                }}
              />
            </div>
          ))}
          <div style={{ fontSize: 11, color: '#999', marginTop: 4 }}>
            Leave blank to match any value for that field.
          </div>
        </div>

        {/* Action */}
        <div style={{ background: '#f7f8fa', borderRadius: 6, padding: 12 }}>
          <div style={{ fontSize: 12, fontWeight: 600, marginBottom: 8, color: '#666' }}>
            Action
          </div>
          <div style={{ marginBottom: 8 }}>
            <label style={{ display: 'block', fontSize: 12, color: '#444', marginBottom: 3 }}>
              Task title template *
            </label>
            <input
              value={taskTitle}
              onChange={e => setTaskTitle(e.target.value)}
              placeholder="e.g. Approve: {{subject}}"
              style={{
                width: '100%',
                padding: '5px 8px',
                borderRadius: 4,
                border: '1px solid #ddd',
                boxSizing: 'border-box',
                fontSize: 13,
              }}
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
              style={{
                width: '100%',
                padding: '5px 8px',
                borderRadius: 4,
                border: '1px solid #ddd',
                boxSizing: 'border-box',
                fontSize: 13,
                resize: 'vertical',
              }}
            />
          </div>
        </div>

        {/* Parse script */}
        <div style={{ background: '#f7f8fa', borderRadius: 6, padding: 12 }}>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 6,
            }}>
            <div>
              <div style={{ fontSize: 12, fontWeight: 600, color: '#444' }}>
                Parse script (Python)
              </div>
              <div style={{ fontSize: 11, color: '#888', marginTop: 1 }}>
                Extracts variables from email body. Use{' '}
                <code style={{ fontSize: 11 }}>{'{{var_name}}'}</code> in templates above.
              </div>
            </div>
            <button
              type="button"
              onClick={() => setShowScript(v => !v)}
              style={{
                fontSize: 11,
                padding: '2px 8px',
                borderRadius: 4,
                border: '1px solid #ccc',
                background: '#fff',
                cursor: 'pointer',
              }}>
              {showScript ? 'Hide' : parseScript ? 'Edit script' : 'Add script'}
            </button>
          </div>
          {showScript && (
            <textarea
              value={parseScript}
              onChange={e => setParseScript(e.target.value)}
              placeholder={
                'import sys, json, re\n\nemail_body = sys.argv[1]\n\n# Extract variables from email body\n# ...\n\nprint(json.dumps({\n    "employee_name": "...",\n    "approval_url": "..."\n}))'
              }
              rows={12}
              spellCheck={false}
              style={{
                width: '100%',
                padding: '8px 10px',
                borderRadius: 4,
                border: '1px solid #ddd',
                fontSize: 12,
                fontFamily: 'JetBrains Mono, Menlo, monospace',
                resize: 'vertical',
                boxSizing: 'border-box',
                background: '#fff',
                color: '#222',
                lineHeight: 1.5,
              }}
            />
          )}
          {!showScript && parseScript && (
            <div style={{ fontSize: 11, color: '#666', fontFamily: 'monospace' }}>
              {parseScript.split('\n').slice(0, 3).join('\n')}
              {parseScript.split('\n').length > 3 ? '\n...' : ''}
            </div>
          )}
        </div>

        {/* Batch mode */}
        <div style={{ background: '#f7f8fa', borderRadius: 6, padding: 12 }}>
          <label
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 8,
              cursor: 'pointer',
              marginBottom: batchMode ? 10 : 0,
            }}>
            <input
              type="checkbox"
              checked={batchMode}
              onChange={e => setBatchMode(e.target.checked)}
            />
            <span style={{ fontSize: 13, fontWeight: 600 }}>Batch mode</span>
            <span style={{ fontSize: 11, color: '#888' }}>
              Accumulate emails and create one combined task
            </span>
          </label>
          {batchMode && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 10, marginTop: 4 }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                <label style={{ fontSize: 12, color: '#444', whiteSpace: 'nowrap' }}>
                  Time window (hours)
                </label>
                <input
                  type="number"
                  min={1}
                  max={168}
                  value={batchWindowHours}
                  onChange={e => setBatchWindowHours(Math.max(1, parseInt(e.target.value) || 1))}
                  style={{
                    width: 70,
                    padding: '4px 8px',
                    borderRadius: 4,
                    border: '1px solid #ddd',
                    fontSize: 13,
                  }}
                />
                <span style={{ fontSize: 11, color: '#888' }}>
                  After the first match, wait this long before creating the task
                </span>
              </div>
              <div>
                <label style={{ display: 'block', fontSize: 12, color: '#444', marginBottom: 4 }}>
                  Parse mode
                </label>
                <div style={{ display: 'flex', gap: 12 }}>
                  {(
                    [
                      [
                        'first_only',
                        'First email only',
                        'Run parse_script on the first matched email; use its vars for the task',
                      ],
                      [
                        'all',
                        'All emails',
                        'Run parse_script on every email; merge results into {{items}} list',
                      ],
                    ] as [BatchParseMode, string, string][]
                  ).map(([val, label, desc]) => (
                    <label
                      key={val}
                      style={{
                        display: 'flex',
                        alignItems: 'flex-start',
                        gap: 6,
                        cursor: 'pointer',
                        flex: 1,
                        padding: '6px 8px',
                        border: `1px solid ${batchParseMode === val ? '#4A83DD' : '#ddd'}`,
                        borderRadius: 6,
                        background: batchParseMode === val ? '#EBF3FF' : '#fff',
                      }}>
                      <input
                        type="radio"
                        name="batchParseMode"
                        value={val}
                        checked={batchParseMode === val}
                        onChange={() => setBatchParseMode(val)}
                        style={{ marginTop: 2 }}
                      />
                      <div>
                        <div style={{ fontSize: 12, fontWeight: 600 }}>{label}</div>
                        <div style={{ fontSize: 11, color: '#888', marginTop: 2 }}>{desc}</div>
                      </div>
                    </label>
                  ))}
                </div>
                {batchParseMode === 'all' && (
                  <div
                    style={{
                      marginTop: 6,
                      fontSize: 11,
                      color: '#666',
                      background: '#f0f4ff',
                      padding: '4px 8px',
                      borderRadius: 4,
                    }}>
                    Use <code>{'{{items}}'}</code> in your description template to include the
                    merged list, and <code>{'{{count}}'}</code> for the number of emails.
                  </div>
                )}
              </div>
            </div>
          )}
        </div>

        {/* Dry Run */}
        <div style={{ background: '#f7f8fa', borderRadius: 6, padding: 12 }}>
          <div
            style={{
              display: 'flex',
              justifyContent: 'space-between',
              alignItems: 'center',
              marginBottom: 6,
            }}>
            <div style={{ fontSize: 12, fontWeight: 600, color: '#444' }}>Dry Run</div>
            <div style={{ display: 'flex', gap: 6 }}>
              <button
                type="button"
                onClick={() => {
                  setDryRunMode('pick');
                  setShowDryRun(true);
                }}
                style={{
                  fontSize: 11,
                  padding: '2px 8px',
                  borderRadius: 4,
                  border: '1px solid #ccc',
                  background: '#fff',
                  cursor: 'pointer',
                }}>
                Pick email
              </button>
              <button
                type="button"
                onClick={() => {
                  setDryRunMode('manual');
                  setShowDryRun(v => !v);
                }}
                style={{
                  fontSize: 11,
                  padding: '2px 8px',
                  borderRadius: 4,
                  border: '1px solid #ccc',
                  background: '#fff',
                  cursor: 'pointer',
                }}>
                {showDryRun && dryRunMode === 'manual' ? 'Hide' : 'Paste body'}
              </button>
            </div>
          </div>

          {showDryRun && dryRunMode === 'manual' && (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
              <textarea
                value={dryRunBody}
                onChange={e => setDryRunBody(e.target.value)}
                placeholder="Paste email body here to test…"
                rows={6}
                style={{
                  width: '100%',
                  padding: '6px 8px',
                  borderRadius: 4,
                  border: '1px solid #ddd',
                  fontSize: 12,
                  fontFamily: 'inherit',
                  resize: 'vertical',
                  boxSizing: 'border-box',
                }}
              />
              <button
                type="button"
                onClick={handleDryRun}
                disabled={dryRunRunning || !dryRunBody.trim()}
                style={{
                  alignSelf: 'flex-end',
                  padding: '5px 14px',
                  borderRadius: 4,
                  border: 'none',
                  background: '#4A83DD',
                  color: '#fff',
                  cursor: dryRunRunning ? 'not-allowed' : 'pointer',
                  fontSize: 12,
                }}>
                {dryRunRunning ? 'Running…' : '▶ Run'}
              </button>
            </div>
          )}

          {dryRunMode === 'pick' && showDryRun && (
            <div style={{ fontSize: 12, color: '#888' }}>
              Select an email from the picker above.
            </div>
          )}

          {dryRunError && (
            <div
              style={{
                marginTop: 6,
                fontSize: 12,
                color: '#d32f2f',
                padding: '4px 6px',
                background: '#fff0f0',
                borderRadius: 4,
              }}>
              {dryRunError}
            </div>
          )}

          {dryRunResult && (
            <div style={{ marginTop: 10, display: 'flex', flexDirection: 'column', gap: 8 }}>
              <div>
                <div style={{ fontSize: 11, fontWeight: 600, color: '#555', marginBottom: 2 }}>
                  Task Title
                </div>
                <div
                  style={{
                    fontSize: 13,
                    padding: '6px 8px',
                    background: '#fff',
                    border: '1px solid #e0e0e0',
                    borderRadius: 4,
                  }}>
                  {dryRunResult.title}
                </div>
              </div>
              {dryRunResult.description && (
                <div>
                  <div style={{ fontSize: 11, fontWeight: 600, color: '#555', marginBottom: 2 }}>
                    Task Description
                  </div>
                  <pre
                    style={{
                      fontSize: 12,
                      padding: '6px 8px',
                      background: '#fff',
                      border: '1px solid #e0e0e0',
                      borderRadius: 4,
                      margin: 0,
                      whiteSpace: 'pre-wrap',
                      fontFamily: 'inherit',
                    }}>
                    {dryRunResult.description}
                  </pre>
                </div>
              )}
              {dryRunResult.parsed_vars && Object.keys(dryRunResult.parsed_vars).length > 0 && (
                <div>
                  <div style={{ fontSize: 11, fontWeight: 600, color: '#555', marginBottom: 2 }}>
                    Extracted variables
                  </div>
                  <pre
                    style={{
                      fontSize: 11,
                      padding: '6px 8px',
                      background: '#f0f4ff',
                      border: '1px solid #c5d0e8',
                      borderRadius: 4,
                      margin: 0,
                      whiteSpace: 'pre-wrap',
                    }}>
                    {JSON.stringify(dryRunResult.parsed_vars, null, 2)}
                  </pre>
                </div>
              )}
              {dryRunResult.script_error && (
                <div
                  style={{
                    fontSize: 12,
                    color: '#d32f2f',
                    padding: '4px 6px',
                    background: '#fff0f0',
                    borderRadius: 4,
                  }}>
                  Script error: {dryRunResult.script_error}
                </div>
              )}

              {/* Refinement chat */}
              <div style={{ marginTop: 10, borderTop: '1px solid #e8e8e8', paddingTop: 10 }}>
                <div style={{ fontSize: 11, fontWeight: 600, color: '#555', marginBottom: 6 }}>
                  Refine with AI
                </div>
                <div style={{ display: 'flex', gap: 6, alignItems: 'flex-end' }}>
                  <textarea
                    value={refineFeedback}
                    onChange={e => setRefineFeedback(e.target.value)}
                    onKeyDown={async e => {
                      if (e.key === 'Enter' && !e.shiftKey && refineFeedback.trim() && !refining) {
                        e.preventDefault();
                        await handleRefine();
                      }
                    }}
                    placeholder="e.g. 'include the leave dates', 'shorten the title'…&#10;Press Enter to send, Shift+Enter for new line"
                    disabled={refining}
                    rows={2}
                    style={{
                      flex: 1,
                      padding: '5px 8px',
                      borderRadius: 4,
                      border: '1px solid #ddd',
                      fontSize: 12,
                      resize: 'vertical',
                      fontFamily: 'inherit',
                      lineHeight: 1.4,
                    }}
                  />
                  <button
                    type="button"
                    onClick={handleRefine}
                    disabled={refining || !refineFeedback.trim()}
                    style={{
                      padding: '5px 12px',
                      borderRadius: 4,
                      border: 'none',
                      background: refineFeedback.trim() ? '#4A83DD' : '#ccc',
                      color: '#fff',
                      cursor: refining ? 'not-allowed' : 'pointer',
                      fontSize: 12,
                      whiteSpace: 'nowrap',
                      alignSelf: 'flex-end',
                    }}>
                    {refining ? 'Refining…' : '↑ Apply'}
                  </button>
                </div>
                <div style={{ fontSize: 11, color: '#aaa', marginTop: 3 }}>
                  Press Enter or click Apply — the rule will be updated and re-run automatically.
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Email picker for dry run */}
        {dryRunMode === 'pick' && showDryRun && (
          <div
            style={{
              position: 'fixed',
              inset: 0,
              zIndex: 60,
              background: 'rgba(0,0,0,0.4)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
            }}>
            <div
              style={{
                background: '#fff',
                borderRadius: 12,
                padding: 24,
                width: 560,
                maxWidth: '95vw',
                maxHeight: '80vh',
                display: 'flex',
                flexDirection: 'column',
                gap: 12,
                boxShadow: '0 8px 32px rgba(0,0,0,0.18)',
              }}>
              <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                <h3 style={{ margin: 0, fontSize: 15 }}>Pick email for dry run</h3>
                <button
                  onClick={() => setShowDryRun(false)}
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
              <p style={{ margin: 0, fontSize: 12, color: '#888' }}>
                Select an email — its full body will be used for the dry run.
              </p>
              <div
                style={{
                  flex: 1,
                  overflowY: 'auto',
                  border: '1px solid #eee',
                  borderRadius: 8,
                  maxHeight: 320,
                }}>
                <DryRunEmailList
                  onSelect={async chunkId => {
                    setShowDryRun(false);
                    setDryRunMode('manual');
                    setDryRunChunkId(chunkId);
                    setDryRunRunning(true);
                    setDryRunResult(null);
                    setDryRunError('');
                    try {
                      const { dryRunRule: drr } =
                        await import('../../services/api/emailAutomationApi');
                      const result = await drr({
                        task_title_template: taskTitle,
                        task_description_template: taskDesc || null,
                        parse_script: parseScript || null,
                        chunk_id: chunkId,
                      });
                      setDryRunResult(result);
                      setShowDryRun(true);
                    } catch (err: unknown) {
                      setDryRunError(err instanceof Error ? err.message : 'Dry run failed');
                      setShowDryRun(true);
                    } finally {
                      setDryRunRunning(false);
                    }
                  }}
                />
              </div>
            </div>
          </div>
        )}

        {error && (
          <div
            style={{
              fontSize: 12,
              color: '#d32f2f',
              padding: '6px 8px',
              background: '#fff0f0',
              borderRadius: 4,
            }}>
            {error}
          </div>
        )}

        <div style={{ display: 'flex', gap: 8, justifyContent: 'flex-end', marginTop: 4 }}>
          <button
            type="button"
            onClick={onCancel}
            style={{
              padding: '6px 14px',
              borderRadius: 4,
              border: '1px solid #ccc',
              background: '#fff',
              cursor: 'pointer',
              fontSize: 13,
            }}>
            Cancel
          </button>
          <button
            type="submit"
            disabled={saving}
            style={{
              padding: '6px 14px',
              borderRadius: 4,
              border: 'none',
              background: '#4A83DD',
              color: '#fff',
              cursor: saving ? 'not-allowed' : 'pointer',
              fontSize: 13,
            }}>
            {saving ? 'Saving…' : 'Save rule'}
          </button>
        </div>
      </form>
    </>
  );
}

function DryRunEmailList({ onSelect }: { onSelect: (chunkId: string) => void }) {
  const [emails, setEmails] = useState<
    import('../../services/api/emailAutomationApi').EmailChunkSummary[]
  >([]);
  const [loading, setLoading] = useState(true);
  const [senderFilter, setSenderFilter] = useState('');
  const [subjectFilter, setSubjectFilter] = useState('');

  const load = async (sender?: string, subject?: string) => {
    setLoading(true);
    try {
      const result = await searchEmailChunks({
        sender_filter: sender?.trim() || undefined,
        subject_filter: subject?.trim() || undefined,
        limit: 10,
      });
      setEmails(result);
    } catch {
      /* ignore */
    } finally {
      setLoading(false);
    }
  };

  useState(() => {
    load();
  });

  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100%' }}>
      <div style={{ display: 'flex', gap: 6, padding: 8, borderBottom: '1px solid #eee' }}>
        <input
          value={senderFilter}
          onChange={e => setSenderFilter(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && load(senderFilter, subjectFilter)}
          placeholder="Sender…"
          style={{
            flex: 1,
            padding: '4px 8px',
            borderRadius: 4,
            border: '1px solid #ddd',
            fontSize: 12,
          }}
        />
        <input
          value={subjectFilter}
          onChange={e => setSubjectFilter(e.target.value)}
          onKeyDown={e => e.key === 'Enter' && load(senderFilter, subjectFilter)}
          placeholder="Subject…"
          style={{
            flex: 1,
            padding: '4px 8px',
            borderRadius: 4,
            border: '1px solid #ddd',
            fontSize: 12,
          }}
        />
        <button
          type="button"
          onClick={() => load(senderFilter, subjectFilter)}
          style={{
            padding: '4px 10px',
            borderRadius: 4,
            border: 'none',
            background: '#4A83DD',
            color: '#fff',
            cursor: 'pointer',
            fontSize: 12,
          }}>
          Go
        </button>
      </div>
      {loading ? (
        <div style={{ padding: 16, textAlign: 'center', color: '#888', fontSize: 12 }}>
          Loading…
        </div>
      ) : (
        emails.map(e => (
          <div
            key={e.chunk_id}
            onClick={() => onSelect(e.chunk_id)}
            style={{ padding: '8px 12px', borderBottom: '1px solid #f0f0f0', cursor: 'pointer' }}
            onMouseEnter={ev => ((ev.currentTarget as HTMLDivElement).style.background = '#f8f9fa')}
            onMouseLeave={ev => ((ev.currentTarget as HTMLDivElement).style.background = '')}>
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span style={{ fontSize: 12, fontWeight: 600 }}>{e.subject || '(no subject)'}</span>
              <span style={{ fontSize: 11, color: '#999' }}>{e.date}</span>
            </div>
            <div style={{ fontSize: 11, color: '#666' }}>{e.sender}</div>
          </div>
        ))
      )}
    </div>
  );
}
