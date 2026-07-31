import { useEffect, useState } from 'react';

import {
  type CreateRuleInput,
  type EmailChunkSummary,
  generateRuleFromEmails,
  searchEmailChunks,
} from '../../services/api/emailAutomationApi';

interface Props {
  onGenerate: (suggestion: CreateRuleInput) => void;
  onCancel: () => void;
}

export function EmailPickerModal({ onGenerate, onCancel }: Props) {
  const [emails, setEmails] = useState<EmailChunkSummary[]>([]);
  const [loading, setLoading] = useState(true);
  const [senderFilter, setSenderFilter] = useState('');
  const [subjectFilter, setSubjectFilter] = useState('');
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [searching, setSearching] = useState(false);
  const [generating, setGenerating] = useState(false);
  const [error, setError] = useState('');

  const load = async (sender?: string, subject?: string) => {
    setSearching(true);
    try {
      const result = await searchEmailChunks({
        sender_filter: sender?.trim() || undefined,
        subject_filter: subject?.trim() || undefined,
        limit: 10,
      });
      setEmails(result);
    } catch {
      // ignore
    } finally {
      setSearching(false);
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const toggleSelect = (chunkId: string) => {
    setSelected(prev => {
      const next = new Set(prev);
      if (next.has(chunkId)) next.delete(chunkId);
      else next.add(chunkId);
      return next;
    });
  };

  const toggleAll = () => {
    if (selected.size === emails.length) {
      setSelected(new Set());
    } else {
      setSelected(new Set(emails.map(e => e.chunk_id)));
    }
  };

  const handleGenerate = async () => {
    if (selected.size === 0) return;
    setGenerating(true);
    setError('');
    try {
      const suggestion = await generateRuleFromEmails(Array.from(selected));
      onGenerate(suggestion);
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Failed to generate rule');
    } finally {
      setGenerating(false);
    }
  };

  return (
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
      onClick={e => {
        if (e.target === e.currentTarget) onCancel();
      }}>
      <div
        style={{
          background: '#fff',
          borderRadius: 12,
          padding: 24,
          width: 580,
          maxWidth: '95vw',
          maxHeight: '80vh',
          display: 'flex',
          flexDirection: 'column',
          gap: 16,
          boxShadow: '0 8px 32px rgba(0,0,0,0.18)',
        }}>
        {/* Header */}
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          <div>
            <h3 style={{ margin: 0, fontSize: 15, fontWeight: 700 }}>Select emails to analyze</h3>
            <p style={{ margin: '2px 0 0', fontSize: 12, color: '#888' }}>
              Select multiple emails of the same type for a more generic rule
            </p>
          </div>
          <button
            onClick={onCancel}
            style={{
              border: 'none',
              background: 'none',
              cursor: 'pointer',
              fontSize: 20,
              color: '#888',
              lineHeight: 1,
            }}>
            ×
          </button>
        </div>

        {/* Filters */}
        <div style={{ display: 'flex', gap: 8 }}>
          <input
            value={senderFilter}
            onChange={e => setSenderFilter(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && load(senderFilter, subjectFilter)}
            placeholder="Sender filter…"
            style={{
              flex: 1,
              padding: '6px 10px',
              borderRadius: 6,
              border: '1px solid #ddd',
              fontSize: 13,
            }}
          />
          <input
            value={subjectFilter}
            onChange={e => setSubjectFilter(e.target.value)}
            onKeyDown={e => e.key === 'Enter' && load(senderFilter, subjectFilter)}
            placeholder="Subject filter…"
            style={{
              flex: 1,
              padding: '6px 10px',
              borderRadius: 6,
              border: '1px solid #ddd',
              fontSize: 13,
            }}
          />
          <button
            onClick={() => load(senderFilter, subjectFilter)}
            disabled={searching}
            style={{
              padding: '6px 14px',
              borderRadius: 6,
              border: 'none',
              background: '#4A83DD',
              color: '#fff',
              cursor: 'pointer',
              fontSize: 13,
            }}>
            {searching ? '…' : 'Search'}
          </button>
        </div>

        {/* Select all row */}
        {emails.length > 0 && (
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '0 2px' }}>
            <input
              type="checkbox"
              checked={selected.size === emails.length && emails.length > 0}
              onChange={toggleAll}
              style={{ cursor: 'pointer' }}
            />
            <span style={{ fontSize: 12, color: '#666' }}>
              Select all ({emails.length}) · {selected.size} selected
            </span>
          </div>
        )}

        {/* Email list */}
        <div style={{ flex: 1, overflowY: 'auto', border: '1px solid #eee', borderRadius: 8 }}>
          {loading ? (
            <div style={{ padding: 24, textAlign: 'center', color: '#888', fontSize: 13 }}>
              Loading…
            </div>
          ) : emails.length === 0 ? (
            <div style={{ padding: 24, textAlign: 'center', color: '#888', fontSize: 13 }}>
              No emails found. Try a different filter or sync sap-mail first.
            </div>
          ) : (
            emails.map(email => {
              const isSelected = selected.has(email.chunk_id);
              return (
                <div
                  key={email.chunk_id}
                  onClick={() => toggleSelect(email.chunk_id)}
                  style={{
                    padding: '10px 14px',
                    borderBottom: '1px solid #f0f0f0',
                    cursor: 'pointer',
                    background: isSelected ? '#EBF3FF' : '#fff',
                    display: 'flex',
                    gap: 10,
                    alignItems: 'flex-start',
                    transition: 'background 0.1s',
                  }}>
                  <input
                    type="checkbox"
                    checked={isSelected}
                    onChange={() => toggleSelect(email.chunk_id)}
                    onClick={e => e.stopPropagation()}
                    style={{ marginTop: 3, cursor: 'pointer', flexShrink: 0 }}
                  />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div
                      style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                      <span
                        style={{
                          fontSize: 13,
                          fontWeight: 600,
                          color: '#222',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}>
                        {email.subject || '(no subject)'}
                      </span>
                      <span style={{ fontSize: 11, color: '#999', flexShrink: 0, marginLeft: 8 }}>
                        {email.date}
                      </span>
                    </div>
                    <div style={{ fontSize: 12, color: '#666' }}>{email.sender}</div>
                    {email.preview && (
                      <div
                        style={{
                          fontSize: 12,
                          color: '#999',
                          marginTop: 2,
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap',
                        }}>
                        {email.preview}
                      </div>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>

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

        {/* Footer */}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <button
            onClick={onCancel}
            style={{
              padding: '6px 14px',
              borderRadius: 6,
              border: '1px solid #ccc',
              background: '#fff',
              cursor: 'pointer',
              fontSize: 13,
            }}>
            Cancel
          </button>
          <button
            onClick={handleGenerate}
            disabled={selected.size === 0 || generating}
            style={{
              padding: '6px 14px',
              borderRadius: 6,
              border: 'none',
              background: selected.size > 0 ? '#4A83DD' : '#ccc',
              color: '#fff',
              cursor: selected.size > 0 && !generating ? 'pointer' : 'not-allowed',
              fontSize: 13,
            }}>
            {generating
              ? 'Generating…'
              : `Generate rule from ${selected.size} email${selected.size !== 1 ? 's' : ''}`}
          </button>
        </div>
      </div>
    </div>
  );
}
