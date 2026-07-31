/**
 * TopicThreadsPanel — manage user-defined topic threads and view each topic's
 * auto-aggregated summary timeline.
 *
 * A topic is defined by keywords (OR/AND), pinned Teams conversations, and
 * pinned people (person/email entities). Matching chunks are routed by the
 * core into the topic's backing summary tree; the timeline here renders the
 * highest-level summary (current state) followed by lower-level history —
 * ready to lift into a status report.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import { BubbleMarkdown } from '../../pages/conversations/components/AgentMessageBubble';
import {
  backfillTopic,
  type CreateTopicInput,
  createTopicThread,
  deleteTopicThread,
  discoverConversations,
  discoverMeetings,
  discoverPeople,
  type KeywordLogic,
  listTopicThreads,
  type MeetingInfo,
  type PersonEntity,
  resolveChatLink,
  type TeamsConversation,
  type TopicThread,
  topicThreadTimeline,
  type TopicTimelineNode,
  updateTopicThread,
} from '../../services/api/topicThreadsApi';

interface FormState {
  name: string;
  description: string;
  keywordLogic: KeywordLogic;
  keywords: string[];
  sourceIds: string[];
  entityIds: string[];
  meetingNames: string[];
}

const EMPTY_FORM: FormState = {
  name: '',
  description: '',
  keywordLogic: 'or',
  keywords: [],
  sourceIds: [],
  entityIds: [],
  meetingNames: [],
};

function fromThread(t: TopicThread): FormState {
  return {
    name: t.name,
    description: t.description,
    keywordLogic: t.keyword_logic,
    keywords: [...t.keywords],
    sourceIds: [...t.source_pins],
    entityIds: [...t.entity_pins],
    meetingNames: [...t.meeting_pins],
  };
}

export function TopicThreadsPanel() {
  const { t } = useT();
  const [threads, setThreads] = useState<TopicThread[]>([]);
  const [conversations, setConversations] = useState<TeamsConversation[]>([]);
  const [people, setPeople] = useState<PersonEntity[]>([]);
  const [meetings, setMeetings] = useState<MeetingInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [selected, setSelected] = useState<TopicThread | null>(null);
  const [creating, setCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setLoading(true);
    Promise.all([
      listTopicThreads(),
      discoverConversations().catch(() => [] as TeamsConversation[]),
      discoverPeople().catch(() => [] as PersonEntity[]),
      discoverMeetings().catch(() => [] as MeetingInfo[]),
    ])
      .then(([ts, convos, ppl, mtgs]) => {
        setThreads(ts);
        setConversations(convos);
        setPeople(ppl);
        setMeetings(mtgs);
      })
      .catch(e => setError(String(e)))
      .finally(() => setLoading(false));
  }, []);

  useEffect(() => {
    load();
  }, [load]);

  const openCreate = () => {
    setSelected(null);
    setCreating(true);
  };

  return (
    <div className="p-6">
      {(creating || selected) && (
        <TopicEditorModal
          initial={selected ? fromThread(selected) : EMPTY_FORM}
          topicId={selected?.id ?? null}
          conversations={conversations}
          people={people}
          meetings={meetings}
          onClose={() => {
            setCreating(false);
            setSelected(null);
          }}
          onSaved={() => {
            setCreating(false);
            setSelected(null);
            load();
          }}
        />
      )}

      <div className="flex items-center justify-between mb-4">
        <div>
          <h2 className="text-sm font-bold text-stone-800 dark:text-neutral-100">
            {t('topics.title')}
          </h2>
          <p className="text-xs text-stone-500 dark:text-neutral-400 mt-0.5">
            {t('topics.subtitle')}
          </p>
        </div>
        <button
          onClick={openCreate}
          className="rounded-lg bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600 transition-colors">
          {t('topics.new')}
        </button>
      </div>

      {error && (
        <p className="text-xs text-coral-500 mb-3" role="alert">
          {error}
        </p>
      )}

      {loading ? (
        <p className="text-sm text-stone-400 dark:text-neutral-500">{t('topics.loading')}</p>
      ) : threads.length === 0 ? (
        <div className="rounded-xl border border-dashed border-stone-300 dark:border-neutral-700 p-8 text-center">
          <p className="text-sm font-medium text-stone-600 dark:text-neutral-300">
            {t('topics.emptyTitle')}
          </p>
          <p className="text-xs text-stone-400 dark:text-neutral-500 mt-1">
            {t('topics.emptyDesc')}
          </p>
        </div>
      ) : (
        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          {threads.map(thread => (
            <button
              key={thread.id}
              onClick={() => setSelected(thread)}
              className="flex flex-col items-start gap-2 rounded-xl border border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-800 p-4 text-left hover:border-primary-400 hover:shadow-sm transition-all">
              <span className="text-sm font-semibold text-stone-800 dark:text-neutral-100 leading-tight">
                {thread.name}
              </span>
              {thread.description && (
                <span className="text-xs text-stone-500 dark:text-neutral-400 line-clamp-2 leading-relaxed">
                  {thread.description}
                </span>
              )}
              <div className="flex flex-wrap gap-1.5 mt-auto pt-1">
                <MetaChip label={`${thread.keywords.length} ${t('topics.chip.keywords')}`} />
                <MetaChip
                  label={`${thread.source_pins.length} ${t('topics.chip.conversations')}`}
                />
                <MetaChip label={`${thread.entity_pins.length} ${t('topics.chip.people')}`} />
                <MetaChip label={`${thread.meeting_pins.length} ${t('topics.chip.meetings')}`} />
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}

function MetaChip({ label }: { label: string }) {
  return (
    <span className="rounded-full bg-stone-100 dark:bg-neutral-700 px-2 py-0.5 text-[10px] font-medium text-stone-600 dark:text-neutral-300">
      {label}
    </span>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Editor + timeline modal
// ─────────────────────────────────────────────────────────────────────────────

interface EditorProps {
  initial: FormState;
  topicId: string | null;
  conversations: TeamsConversation[];
  people: PersonEntity[];
  meetings: MeetingInfo[];
  onClose: () => void;
  onSaved: () => void;
}

function TopicEditorModal({
  initial,
  topicId,
  conversations,
  people,
  meetings,
  onClose,
  onSaved,
}: EditorProps) {
  const { t } = useT();
  const [form, setForm] = useState<FormState>(initial);
  const [keywordDraft, setKeywordDraft] = useState('');
  const [meetingDraft, setMeetingDraft] = useState('');
  const [backfillDays, setBackfillDays] = useState<number>(0);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Conversations resolved from pasted Teams links this session, merged into
  // the discovered list so they show up in the picker immediately.
  const [pastedConvos, setPastedConvos] = useState<TeamsConversation[]>([]);
  const [linkDraft, setLinkDraft] = useState('');
  const [resolving, setResolving] = useState(false);

  const allConversations = useMemo(() => {
    const byPin = new Map<string, TeamsConversation>();
    for (const c of conversations) byPin.set(c.pin_value, c);
    for (const c of pastedConvos) byPin.set(c.pin_value, c);
    return Array.from(byPin.values());
  }, [conversations, pastedConvos]);

  const handleResolveLink = async () => {
    const url = linkDraft.trim();
    if (!url) return;
    setResolving(true);
    setError(null);
    try {
      const convo = await resolveChatLink(url);
      setPastedConvos(prev =>
        prev.some(c => c.pin_value === convo.pin_value) ? prev : [...prev, convo]
      );
      // Auto-select the just-added conversation.
      setForm(f => ({
        ...f,
        sourceIds: f.sourceIds.includes(convo.pin_value)
          ? f.sourceIds
          : [...f.sourceIds, convo.pin_value],
      }));
      setLinkDraft('');
    } catch (e) {
      setError(String(e));
    } finally {
      setResolving(false);
    }
  };

  const isEdit = topicId != null;

  const addKeyword = () => {
    const v = keywordDraft.trim();
    if (v && !form.keywords.includes(v)) {
      setForm(f => ({ ...f, keywords: [...f.keywords, v] }));
    }
    setKeywordDraft('');
  };

  const toggleConversation = (pinValue: string) => {
    setForm(f => ({
      ...f,
      sourceIds: f.sourceIds.includes(pinValue)
        ? f.sourceIds.filter(s => s !== pinValue)
        : [...f.sourceIds, pinValue],
    }));
  };

  const togglePerson = (entityId: string) => {
    setForm(f => ({
      ...f,
      entityIds: f.entityIds.includes(entityId)
        ? f.entityIds.filter(e => e !== entityId)
        : [...f.entityIds, entityId],
    }));
  };

  const addMeeting = () => {
    const v = meetingDraft.trim();
    if (v && !form.meetingNames.includes(v)) {
      setForm(f => ({ ...f, meetingNames: [...f.meetingNames, v] }));
    }
    setMeetingDraft('');
  };

  const toggleMeeting = (name: string) => {
    setForm(f => ({
      ...f,
      meetingNames: f.meetingNames.includes(name)
        ? f.meetingNames.filter(m => m !== name)
        : [...f.meetingNames, name],
    }));
  };

  const handleSave = async () => {
    if (!form.name.trim()) {
      setError(t('topics.err.nameRequired'));
      return;
    }
    setSaving(true);
    setError(null);
    const payload: CreateTopicInput = {
      name: form.name.trim(),
      description: form.description.trim(),
      keyword_logic: form.keywordLogic,
      keywords: form.keywords,
      source_ids: form.sourceIds,
      entity_ids: form.entityIds,
      meeting_names: form.meetingNames,
      ...(!isEdit && backfillDays > 0 ? { backfill_days: backfillDays } : {}),
    };
    try {
      if (isEdit && topicId) {
        await updateTopicThread(topicId, payload);
      } else {
        await createTopicThread(payload);
      }
      onSaved();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!topicId) return;
    if (!window.confirm(t('topics.confirmDelete'))) return;
    setSaving(true);
    try {
      await deleteTopicThread(topicId);
      onSaved();
    } catch (e) {
      setError(String(e));
      setSaving(false);
    }
  };

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/40 backdrop-blur-sm"
      onClick={onClose}>
      <div
        className="bg-white dark:bg-neutral-900 rounded-2xl shadow-2xl w-full max-w-3xl mx-4 max-h-[88vh] flex flex-col"
        onClick={e => e.stopPropagation()}>
        <div className="flex items-center justify-between px-5 py-3 border-b border-stone-200 dark:border-neutral-700">
          <h3 className="text-sm font-bold text-stone-800 dark:text-neutral-100">
            {isEdit ? t('topics.editTitle') : t('topics.newTitle')}
          </h3>
          <button
            onClick={onClose}
            className="text-stone-400 hover:text-stone-600 dark:hover:text-neutral-200 text-xl leading-none px-1">
            ×
          </button>
        </div>

        <div className="flex-1 overflow-y-auto px-5 py-4 space-y-4 text-sm">
          {/* Definition */}
          <div className="space-y-3">
            <Field label={t('topics.field.name')}>
              <input
                value={form.name}
                onChange={e => setForm(f => ({ ...f, name: e.target.value }))}
                className="w-full rounded-lg border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-3 py-1.5 text-sm"
                placeholder={t('topics.field.namePlaceholder')}
              />
            </Field>
            <Field label={t('topics.field.description')}>
              <textarea
                value={form.description}
                onChange={e => setForm(f => ({ ...f, description: e.target.value }))}
                rows={2}
                className="w-full rounded-lg border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-3 py-1.5 text-sm resize-none"
                placeholder={t('topics.field.descriptionPlaceholder')}
              />
            </Field>

            {/* Keyword logic */}
            <Field label={t('topics.field.keywordLogic')}>
              <div className="flex gap-3">
                {(['or', 'and'] as KeywordLogic[]).map(logic => (
                  <label key={logic} className="flex items-center gap-1.5 text-xs cursor-pointer">
                    <input
                      type="radio"
                      name="keywordLogic"
                      checked={form.keywordLogic === logic}
                      onChange={() => setForm(f => ({ ...f, keywordLogic: logic }))}
                    />
                    {logic === 'or' ? t('topics.logic.or') : t('topics.logic.and')}
                  </label>
                ))}
              </div>
            </Field>

            {/* Keywords */}
            <Field label={t('topics.field.keywords')}>
              <TagInput
                draft={keywordDraft}
                setDraft={setKeywordDraft}
                onAdd={addKeyword}
                placeholder={t('topics.field.keywordsPlaceholder')}
                tags={form.keywords}
                onRemove={kw =>
                  setForm(f => ({ ...f, keywords: f.keywords.filter(k => k !== kw) }))
                }
              />
            </Field>

            {/* Conversation pins (Teams 1:1 + group chats) */}
            <Field label={t('topics.field.conversations')}>
              {/* Paste a Teams chat link to add a conversation without syncing first. */}
              <div className="flex gap-2 mb-2">
                <input
                  value={linkDraft}
                  onChange={e => setLinkDraft(e.target.value)}
                  onKeyDown={e => {
                    if (e.key === 'Enter') {
                      e.preventDefault();
                      void handleResolveLink();
                    }
                  }}
                  className="flex-1 rounded-lg border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-3 py-1.5 text-xs"
                  placeholder={t('topics.field.pasteLink')}
                />
                <button
                  type="button"
                  onClick={() => void handleResolveLink()}
                  disabled={resolving || !linkDraft.trim()}
                  className="rounded-lg bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50 whitespace-nowrap">
                  {resolving ? t('topics.field.resolving') : t('topics.field.addLink')}
                </button>
              </div>
              <PickerList
                items={allConversations.map(c => ({
                  value: c.pin_value,
                  label: c.label,
                  sub: c.chat_type ?? undefined,
                }))}
                selected={form.sourceIds}
                onToggle={toggleConversation}
                emptyText={t('topics.field.noConversations')}
                searchPlaceholder={t('topics.field.searchConversations')}
              />
            </Field>

            {/* People pins (person / email entities) */}
            <Field label={t('topics.field.people')}>
              <PickerList
                items={people.map(p => ({
                  value: p.entity_id,
                  label: p.surface,
                  sub: `${p.kind} · ${p.count}`,
                }))}
                selected={form.entityIds}
                onToggle={togglePerson}
                emptyText={t('topics.field.noPeople')}
                searchPlaceholder={t('topics.field.searchPeople')}
              />
            </Field>

            {/* Meeting pins — direct substring input + discovered-meeting picker */}
            <Field label={t('topics.field.meetings')}>
              <TagInput
                draft={meetingDraft}
                setDraft={setMeetingDraft}
                onAdd={addMeeting}
                placeholder={t('topics.field.meetingsPlaceholder')}
                tags={form.meetingNames}
                onRemove={m =>
                  setForm(f => ({ ...f, meetingNames: f.meetingNames.filter(x => x !== m) }))
                }
              />
              <div className="mt-2">
                <PickerList
                  items={meetings.map(m => ({
                    value: m.meeting_name,
                    label: m.meeting_name,
                    sub: `${m.count}`,
                  }))}
                  selected={form.meetingNames}
                  onToggle={toggleMeeting}
                  emptyText={t('topics.field.noMeetings')}
                  searchPlaceholder={t('topics.field.searchMeetings')}
                />
              </div>
            </Field>

            {/* Backfill window — create mode only */}
            {!isEdit && (
              <Field label={t('topics.field.backfill')}>
                <div className="flex gap-3">
                  {[0, 7, 14, 30].map(d => (
                    <label key={d} className="flex items-center gap-1.5 text-xs cursor-pointer">
                      <input
                        type="radio"
                        name="backfillDays"
                        checked={backfillDays === d}
                        onChange={() => setBackfillDays(d)}
                      />
                      {d === 0 ? t('topics.backfill.none') : `${d}d`}
                    </label>
                  ))}
                </div>
              </Field>
            )}
          </div>

          {/* Timeline + backfill (edit mode only) */}
          {isEdit && topicId && <TopicTimeline topicId={topicId} />}
        </div>

        {error && (
          <p className="px-5 text-xs text-coral-500" role="alert">
            {error}
          </p>
        )}

        <div className="flex items-center justify-between px-5 py-3 border-t border-stone-200 dark:border-neutral-700">
          {isEdit ? (
            <button
              onClick={handleDelete}
              disabled={saving}
              className="text-xs font-medium text-coral-500 hover:text-coral-600 disabled:opacity-50">
              {t('topics.delete')}
            </button>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="rounded-lg px-3 py-1.5 text-xs font-medium text-stone-600 dark:text-neutral-300 hover:bg-stone-100 dark:hover:bg-neutral-800">
              {t('topics.cancel')}
            </button>
            <button
              onClick={handleSave}
              disabled={saving}
              className="rounded-lg bg-primary-500 px-3 py-1.5 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50">
              {saving ? t('topics.saving') : t('topics.save')}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="block">
      <span className="block text-xs font-semibold text-stone-600 dark:text-neutral-300 mb-1">
        {label}
      </span>
      {children}
    </label>
  );
}

function TagInput({
  draft,
  setDraft,
  onAdd,
  placeholder,
  tags,
  onRemove,
}: {
  draft: string;
  setDraft: (v: string) => void;
  onAdd: () => void;
  placeholder: string;
  tags: string[];
  onRemove: (tag: string) => void;
}) {
  return (
    <div>
      <input
        value={draft}
        onChange={e => setDraft(e.target.value)}
        onKeyDown={e => {
          if (e.key === 'Enter' || e.key === ',') {
            e.preventDefault();
            onAdd();
          }
        }}
        onBlur={onAdd}
        className="w-full rounded-lg border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-3 py-1.5 text-sm"
        placeholder={placeholder}
      />
      {tags.length > 0 && (
        <div className="flex flex-wrap gap-1.5 mt-2">
          {tags.map(tag => (
            <span
              key={tag}
              className="flex items-center gap-1 rounded-full bg-stone-100 dark:bg-neutral-700 px-2 py-0.5 text-[11px] text-stone-600 dark:text-neutral-300">
              {tag}
              <button
                type="button"
                onClick={() => onRemove(tag)}
                className="text-stone-400 hover:text-coral-500 leading-none">
                ×
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

interface PickerItem {
  value: string;
  label: string;
  sub?: string;
}

/** Searchable multi-select list for conversations / people pins. */
function PickerList({
  items,
  selected,
  onToggle,
  emptyText,
  searchPlaceholder,
}: {
  items: PickerItem[];
  selected: string[];
  onToggle: (value: string) => void;
  emptyText: string;
  searchPlaceholder: string;
}) {
  const [query, setQuery] = useState('');
  const q = query.trim().toLowerCase();
  const filtered = useMemo(
    () => (q ? items.filter(it => it.label.toLowerCase().includes(q)) : items),
    [items, q]
  );

  if (items.length === 0) {
    return <p className="text-xs text-stone-400 dark:text-neutral-500">{emptyText}</p>;
  }

  return (
    <div>
      <input
        value={query}
        onChange={e => setQuery(e.target.value)}
        className="w-full rounded-lg border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-3 py-1.5 text-sm mb-2"
        placeholder={searchPlaceholder}
      />
      <div className="max-h-40 overflow-y-auto rounded-lg border border-stone-200 dark:border-neutral-700 divide-y divide-stone-100 dark:divide-neutral-800">
        {filtered.map(it => {
          const active = selected.includes(it.value);
          return (
            <button
              key={it.value}
              type="button"
              onClick={() => onToggle(it.value)}
              className={`flex w-full items-center justify-between gap-2 px-3 py-1.5 text-left text-xs transition-colors ${
                active
                  ? 'bg-primary-50 dark:bg-primary-900/20'
                  : 'hover:bg-stone-50 dark:hover:bg-neutral-800'
              }`}>
              <span className="flex-1 truncate">
                <span className="text-stone-700 dark:text-neutral-200">{it.label}</span>
                {it.sub && (
                  <span className="ml-2 text-[10px] text-stone-400 dark:text-neutral-500">
                    {it.sub}
                  </span>
                )}
              </span>
              {active && <span className="text-primary-500 text-sm leading-none">✓</span>}
            </button>
          );
        })}
        {filtered.length === 0 && (
          <p className="px-3 py-2 text-[11px] text-stone-400 dark:text-neutral-500">—</p>
        )}
      </div>
      {selected.length > 0 && (
        <p className="mt-1 text-[10px] text-stone-400 dark:text-neutral-500">
          {selected.length} selected
        </p>
      )}
    </div>
  );
}

function TopicTimeline({ topicId }: { topicId: string }) {
  const { t } = useT();
  const [nodes, setNodes] = useState<TopicTimelineNode[] | null>(null);
  const [loading, setLoading] = useState(true);
  const [backfilling, setBackfilling] = useState(false);
  const [backfillMsg, setBackfillMsg] = useState<string | null>(null);

  const reload = useCallback(() => {
    setLoading(true);
    topicThreadTimeline(topicId)
      .then(setNodes)
      .catch(() => setNodes([]))
      .finally(() => setLoading(false));
  }, [topicId]);

  useEffect(() => {
    reload();
  }, [reload]);

  const runBackfill = async (days: number) => {
    setBackfilling(true);
    setBackfillMsg(null);
    try {
      const r = await backfillTopic(topicId, days);
      setBackfillMsg(t('topics.backfill.done'));
      // Give the seal pipeline a moment, then refresh.
      setTimeout(reload, 1500);
      void r;
    } catch (e) {
      setBackfillMsg(String(e));
    } finally {
      setBackfilling(false);
    }
  };

  const sorted = useMemo(() => nodes ?? [], [nodes]);

  return (
    <div className="border-t border-stone-200 dark:border-neutral-700 pt-4">
      <div className="flex items-center justify-between mb-3">
        <h4 className="text-xs font-bold uppercase tracking-wide text-stone-500 dark:text-neutral-400">
          {t('topics.timeline')}
        </h4>
        <div className="flex items-center gap-1.5">
          <span className="text-[10px] text-stone-400 dark:text-neutral-500">
            {t('topics.backfill.label')}
          </span>
          {[7, 14, 30].map(d => (
            <button
              key={d}
              type="button"
              disabled={backfilling}
              onClick={() => void runBackfill(d)}
              className="rounded-md border border-stone-300 dark:border-neutral-600 px-2 py-0.5 text-[10px] font-medium text-stone-600 dark:text-neutral-300 hover:bg-stone-100 dark:hover:bg-neutral-800 disabled:opacity-50">
              {d}d
            </button>
          ))}
        </div>
      </div>
      {backfillMsg && (
        <p className="text-[10px] text-stone-400 dark:text-neutral-500 mb-2">{backfillMsg}</p>
      )}
      {loading ? (
        <p className="text-xs text-stone-400 dark:text-neutral-500">{t('topics.loading')}</p>
      ) : sorted.length === 0 ? (
        <p className="text-xs text-stone-400 dark:text-neutral-500">{t('topics.timelineEmpty')}</p>
      ) : (
        <div className="space-y-3">
          {sorted.map((node, idx) => (
            <div
              key={node.summary_id}
              className={`rounded-lg border p-3 ${
                idx === 0
                  ? 'border-primary-300 bg-primary-50/50 dark:border-primary-700 dark:bg-primary-900/10'
                  : 'border-stone-200 dark:border-neutral-700 bg-white dark:bg-neutral-800'
              }`}>
              <div className="flex items-center gap-2 mb-1.5">
                {idx === 0 && (
                  <span className="rounded-full bg-primary-500 px-2 py-0.5 text-[9px] font-bold uppercase text-white">
                    {t('topics.current')}
                  </span>
                )}
                <span className="text-[10px] font-medium text-stone-400 dark:text-neutral-500">
                  L{node.level} · {new Date(node.time_range_end_ms).toLocaleDateString()}
                </span>
              </div>
              <div className="text-xs">
                <BubbleMarkdown content={node.body} tone="agent" />
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
