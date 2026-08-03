import { useEffect, useRef, useState } from 'react';

import { useT } from '../../../lib/i18n/I18nContext';
import {
  addProfile,
  availableTiers,
  encodeStep,
  getGlobalFallback,
  getLadder,
  type GlobalFallback,
  type LadderStepResolved,
  listProfiles,
  previewModels,
  type ProfileModels,
  type ProfileWithModels,
  removeProfile,
  setGlobalFallback,
  setLadder,
} from '../../../services/api/claudeProfilesApi';
import { isTauri } from '../../../utils/tauriCommands';
import PanelPage from '../../layout/PanelPage';
import SettingsBackButton from '../components/SettingsBackButton';
import {
  SettingsBadge,
  SettingsEmptyState,
  SettingsRow,
  SettingsSection,
  SettingsTextField,
} from '../controls';
import { useSettingsNavigation } from '../hooks/useSettingsNavigation';

const ModelBadges = ({ models }: { models: ProfileModels }) => {
  const tiers = availableTiers(models);
  if (tiers.length === 0) return null;
  return (
    <div className="flex flex-wrap gap-1.5">
      {tiers.map(k => (
        <SettingsBadge key={k} variant="neutral">
          {k}: {models[k]}
        </SettingsBadge>
      ))}
    </div>
  );
};

const ClaudeProfilesPanel = () => {
  const { t } = useT();
  const { navigateBack } = useSettingsNavigation();

  const [profiles, setProfiles] = useState<ProfileWithModels[]>([]);
  const [loading, setLoading] = useState(isTauri());
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [path, setPath] = useState('');
  const [saving, setSaving] = useState(false);

  // Live preview of models parsed from the typed path.
  const [preview, setPreview] = useState<{ models: ProfileModels; readable: boolean } | null>(null);
  const [previewing, setPreviewing] = useState(false);
  const previewTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  // Fallback ladder (ordered).
  const [ladder, setLadderState] = useState<LadderStepResolved[]>([]);

  // Global default fallback (for tasks without a profile).
  const [gf, setGf] = useState<GlobalFallback>({ enabled: false });

  const reload = async () => {
    try {
      const [ps, l, g] = await Promise.all([listProfiles(), getLadder(), getGlobalFallback()]);
      setProfiles(ps);
      setLadderState(l);
      setGf(g);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  };

  // Persist the global default fallback and refresh.
  const commitGf = async (next: GlobalFallback) => {
    setGf(next);
    setError(null);
    try {
      await setGlobalFallback(next);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  // Persist a reordered/edited ladder and refresh from the server.
  const commitLadder = async (next: LadderStepResolved[]) => {
    setLadderState(next);
    setError(null);
    try {
      await setLadder(next.map(s => ({ profile_id: s.profile_id, tier: s.tier })));
      setLadderState(await getLadder());
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  const moveStep = (idx: number, delta: number) => {
    const next = [...ladder];
    const j = idx + delta;
    if (j < 0 || j >= next.length) return;
    [next[idx], next[j]] = [next[j], next[idx]];
    void commitLadder(next);
  };

  const removeStep = (idx: number) => {
    void commitLadder(ladder.filter((_, i) => i !== idx));
  };

  useEffect(() => {
    if (isTauri()) void reload();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Debounced live preview whenever the path changes.
  useEffect(() => {
    if (previewTimer.current) clearTimeout(previewTimer.current);
    const trimmed = path.trim();
    if (!trimmed) {
      setPreview(null);
      setPreviewing(false);
      return;
    }
    setPreviewing(true);
    previewTimer.current = setTimeout(() => {
      previewModels(trimmed)
        .then(r => setPreview(r))
        .catch(() => setPreview(null))
        .finally(() => setPreviewing(false));
    }, 400);
    return () => {
      if (previewTimer.current) clearTimeout(previewTimer.current);
    };
  }, [path]);

  const previewHasModels = !!preview && availableTiers(preview.models).length > 0;

  const handleAdd = async () => {
    if (!name.trim() || !path.trim()) return;
    setSaving(true);
    setError(null);
    try {
      await addProfile(name.trim(), path.trim());
      setName('');
      setPath('');
      setPreview(null);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSaving(false);
    }
  };

  const handleRemove = async (id: string) => {
    setError(null);
    try {
      await removeProfile(id);
      await reload();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    }
  };

  if (!isTauri()) {
    return (
      <PanelPage
        className="z-10"
        contentClassName=""
        description={t('claudeProfiles.menuDesc', 'Claude Code settings profiles')}
        leading={<SettingsBackButton onBack={navigateBack} />}>
        <div className="p-4 pt-2">
          <p className="text-sm text-neutral-500 dark:text-neutral-400">
            {t('claudeProfiles.desktopOnly', 'Available on desktop only.')}
          </p>
        </div>
      </PanelPage>
    );
  }

  return (
    <PanelPage
      className="z-10"
      contentClassName=""
      description={t('claudeProfiles.menuDesc', 'Claude Code settings profiles')}
      leading={<SettingsBackButton onBack={navigateBack} />}>
      <div className="p-4 pt-2 space-y-5">
        <p className="text-sm text-neutral-500 dark:text-neutral-400">
          {t(
            'claudeProfiles.intro',
            'Register your Claude Code settings.json files. Tasks can pick a profile + model; the AI run launches with `claude --settings <path>`. Auth tokens in the files are never read or shown.'
          )}
        </p>

        {/* Registered profiles */}
        <SettingsSection title={t('claudeProfiles.registered', 'Registered profiles')}>
          {loading ? (
            <SettingsEmptyState label={t('claudeProfiles.loading', 'Loading…')} />
          ) : profiles.length === 0 ? (
            <SettingsEmptyState
              label={t('claudeProfiles.empty', 'No profiles yet — add one below.')}
            />
          ) : (
            <div className="flex flex-col gap-2">
              {profiles.map(p => (
                <div
                  key={p.profile.id}
                  className="rounded-lg border border-stone-200 dark:border-neutral-700 p-3">
                  <div className="flex items-start justify-between gap-3">
                    <div className="min-w-0">
                      <div className="text-sm font-medium text-stone-800 dark:text-neutral-100">
                        {p.profile.name}
                      </div>
                      <div className="text-xs text-neutral-500 dark:text-neutral-400 truncate font-mono">
                        {p.profile.path}
                      </div>
                    </div>
                    <button
                      type="button"
                      className="shrink-0 text-xs text-coral-600 dark:text-coral-300 hover:underline"
                      onClick={() => void handleRemove(p.profile.id)}>
                      {t('claudeProfiles.remove', 'Remove')}
                    </button>
                  </div>
                  <div className="mt-2">
                    {p.readable ? (
                      <ModelBadges models={p.models} />
                    ) : (
                      <SettingsBadge variant="warning">
                        {t('claudeProfiles.unreadable', 'file unreadable')}
                      </SettingsBadge>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </SettingsSection>

        {/* Add profile */}
        <SettingsSection
          title={t('claudeProfiles.addTitle', 'Add a profile')}
          description={t(
            'claudeProfiles.addDesc',
            'Point to a settings.json file, e.g. ~/.claude/settings.json.hyperspace'
          )}>
          <SettingsRow
            stacked
            label={t('claudeProfiles.nameLabel', 'Name')}
            control={
              <SettingsTextField
                value={name}
                onChange={e => setName(e.target.value)}
                placeholder={t('claudeProfiles.namePlaceholder', 'e.g. Hyperspace')}
              />
            }
          />
          <SettingsRow
            stacked
            label={t('claudeProfiles.pathLabel', 'Settings file path')}
            control={
              <div className="flex flex-col gap-1.5">
                <SettingsTextField
                  value={path}
                  onChange={e => setPath(e.target.value)}
                  placeholder="/Users/you/.claude/settings.json.hyperspace"
                />
                {/* Live preview of parsed models */}
                {path.trim() && (
                  <div className="text-xs">
                    {previewing ? (
                      <span className="text-neutral-400">
                        {t('claudeProfiles.parsing', 'Parsing…')}
                      </span>
                    ) : previewHasModels ? (
                      <ModelBadges models={preview!.models} />
                    ) : preview && !preview.readable ? (
                      <span className="text-amber-600 dark:text-amber-400">
                        {t('claudeProfiles.previewUnreadable', 'File not found or unreadable.')}
                      </span>
                    ) : (
                      <span className="text-amber-600 dark:text-amber-400">
                        {t('claudeProfiles.previewNoModels', 'No model keys found in this file.')}
                      </span>
                    )}
                  </div>
                )}
              </div>
            }
          />
          <button
            type="button"
            disabled={saving || !name.trim() || !path.trim()}
            className="mt-1 self-start rounded-md bg-primary-500 px-3 py-1.5 text-sm font-medium text-white hover:bg-primary-600 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            onClick={() => void handleAdd()}>
            {saving
              ? t('claudeProfiles.adding', 'Adding…')
              : t('claudeProfiles.add', 'Add profile')}
          </button>
        </SettingsSection>

        {/* Fallback ladder */}
        <SettingsSection
          title={t('claudeProfiles.ladderTitle', 'Fallback ladder')}
          description={t(
            'claudeProfiles.ladderDesc',
            'Global model order used by task fallback. A task falls back along this ladder (up or down) when its model fails to start.'
          )}>
          {ladder.length === 0 ? (
            <SettingsEmptyState
              label={t('claudeProfiles.ladderEmpty', 'Add a profile to build the ladder.')}
            />
          ) : (
            <div className="flex flex-col gap-1.5">
              {ladder.map((s, idx) => (
                <div
                  key={`${s.profile_id}:${s.tier}`}
                  className="flex items-center justify-between gap-3 rounded-md border border-stone-200 dark:border-neutral-700 px-3 py-1.5">
                  <div className="min-w-0 flex items-center gap-2">
                    <span className="text-xs text-neutral-400 tabular-nums">{idx + 1}</span>
                    <span className="text-sm text-stone-800 dark:text-neutral-100">
                      {s.profile_name}
                    </span>
                    <SettingsBadge variant={s.readable && s.model ? 'neutral' : 'warning'}>
                      {s.tier}
                      {s.model ? `: ${s.model}` : ''}
                    </SettingsBadge>
                  </div>
                  <div className="flex items-center gap-1 shrink-0">
                    <button
                      type="button"
                      aria-label="move up"
                      disabled={idx === 0}
                      className="px-1.5 text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 disabled:opacity-30"
                      onClick={() => moveStep(idx, -1)}>
                      ↑
                    </button>
                    <button
                      type="button"
                      aria-label="move down"
                      disabled={idx === ladder.length - 1}
                      className="px-1.5 text-neutral-500 hover:text-neutral-800 dark:hover:text-neutral-200 disabled:opacity-30"
                      onClick={() => moveStep(idx, 1)}>
                      ↓
                    </button>
                    <button
                      type="button"
                      className="px-1.5 text-xs text-coral-600 dark:text-coral-300 hover:underline"
                      onClick={() => removeStep(idx)}>
                      {t('claudeProfiles.remove', 'Remove')}
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </SettingsSection>

        {/* Global default fallback (tasks without a profile) */}
        <SettingsSection
          title={t('claudeProfiles.globalTitle', 'Global default (tasks without a profile)')}
          description={t(
            'claudeProfiles.globalDesc',
            'Tasks that do not pick their own profile use this fallback. Off = they run with the app default model.'
          )}>
          <label className="flex items-center gap-2 text-sm text-stone-700 dark:text-neutral-200">
            <input
              type="checkbox"
              checked={gf.enabled}
              onChange={e =>
                void commitGf(
                  e.target.checked
                    ? { ...gf, enabled: true, direction: gf.direction ?? 'down' }
                    : { ...gf, enabled: false }
                )
              }
            />
            {t('claudeProfiles.globalEnable', 'Enable global default fallback')}
          </label>

          {gf.enabled && ladder.length > 0 && (
            <div className="mt-3 flex flex-wrap items-center gap-x-1.5 gap-y-2 text-sm text-stone-600 dark:text-neutral-300 leading-7">
              <span>{t('claudeProfiles.globalSentStart', 'Start at')}</span>
              <select
                className="rounded-md border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-2 py-1 text-sm text-stone-800 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-primary-500/40 focus:border-primary-500"
                value={
                  gf.start_profile && gf.start_tier
                    ? encodeStep(gf.start_profile, gf.start_tier)
                    : ''
                }
                onChange={e => {
                  const [p, tier] = e.target.value.split(':');
                  void commitGf({
                    ...gf,
                    start_profile: p || undefined,
                    start_tier: tier || undefined,
                  });
                }}>
                <option value="">{t('claudeProfiles.globalStart', 'Start model…')}</option>
                {ladder.map(s => (
                  <option
                    key={`${s.profile_id}:${s.tier}`}
                    value={encodeStep(s.profile_id, s.tier)}>
                    {s.profile_name} / {s.tier}
                  </option>
                ))}
              </select>

              <span>{t('claudeProfiles.globalSentStep', 'and step')}</span>
              <select
                className="rounded-md border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-2 py-1 text-sm text-stone-800 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-primary-500/40 focus:border-primary-500"
                value={gf.direction ?? 'down'}
                onChange={e => void commitGf({ ...gf, direction: e.target.value })}>
                <option value="down">{t('claudeProfiles.globalSentDown', 'down')}</option>
                <option value="up">{t('claudeProfiles.globalSentUp', 'up')}</option>
              </select>

              <span>{t('claudeProfiles.globalSentEnd', 'the ladder, ending at')}</span>
              <select
                className="rounded-md border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 px-2 py-1 text-sm text-stone-800 dark:text-neutral-100 focus:outline-none focus:ring-2 focus:ring-primary-500/40 focus:border-primary-500"
                value={gf.end ?? ''}
                onChange={e => void commitGf({ ...gf, end: e.target.value || undefined })}>
                <option value="">
                  {t('claudeProfiles.globalSentBoundary', 'the ladder boundary')}
                </option>
                {ladder.map(s => (
                  <option
                    key={`${s.profile_id}:${s.tier}`}
                    value={encodeStep(s.profile_id, s.tier)}>
                    {s.profile_name} / {s.tier}
                  </option>
                ))}
              </select>
              <span>.</span>
            </div>
          )}
          {gf.enabled && ladder.length === 0 && (
            <p className="mt-2 text-xs text-amber-600 dark:text-amber-400">
              {t('claudeProfiles.globalNoLadder', 'Add a profile first to build the ladder.')}
            </p>
          )}
        </SettingsSection>

        {error && <p className="text-xs text-coral-600 dark:text-coral-300">{error}</p>}
      </div>
    </PanelPage>
  );
};

export default ClaudeProfilesPanel;
