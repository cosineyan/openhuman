/**
 * ProfileModelPicker — a reusable (Claude settings profile + model) selector.
 *
 * Used in every task-creation surface (project new-task, email rule form,
 * scheduled task form). On mount it loads the registered profiles; selecting a
 * profile reveals a second dropdown of that profile's available model tiers
 * (opus/sonnet/haiku/default). The "Default (no profile)" option yields
 * `{}` → the task runs with the app's default provider auth (legacy behavior).
 *
 * Never renders any auth token — the API never returns one.
 */
import { useEffect, useState } from 'react';

import { useT } from '../../lib/i18n/I18nContext';
import {
  availableTiers,
  encodeStep,
  getLadder,
  type LadderStepResolved,
  listProfiles,
  type ProfileModels,
  type ProfileWithModels,
} from '../../services/api/claudeProfilesApi';

export interface ProfileModelValue {
  settingsProfile?: string;
  model?: string;
  /** "up" | "down" | undefined (fallback off). */
  fallbackDirection?: string;
  /** Encoded "<profile_id>:<tier>" terminus, or undefined (walk to boundary). */
  fallbackEnd?: string;
}

interface Props {
  value: ProfileModelValue;
  onChange: (next: ProfileModelValue) => void;
  /** Optional inline style override for the two <select>s. */
  selectStyle?: React.CSSProperties;
  /** Show the fallback (direction + terminus) controls. Default true. */
  showFallback?: boolean;
}

const tierLabel = (tier: keyof ProfileModels, models: ProfileModels): string =>
  `${tier} (${models[tier]})`;

export function ProfileModelPicker({ value, onChange, selectStyle, showFallback = true }: Props) {
  const { t } = useT();
  const [profiles, setProfiles] = useState<ProfileWithModels[]>([]);
  const [ladder, setLadder] = useState<LadderStepResolved[]>([]);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    getLadder()
      .then(l => {
        if (!cancelled) setLadder(l);
      })
      .catch(() => {
        if (!cancelled) setLadder([]);
      });
    listProfiles()
      .then(p => {
        if (!cancelled) setProfiles(p);
      })
      .catch(() => {
        if (!cancelled) setProfiles([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const selected = profiles.find(p => p.profile.id === value.settingsProfile);
  const tiers = selected ? availableTiers(selected.models) : [];

  const selectClass =
    'rounded-md border border-stone-300 dark:border-neutral-600 bg-white dark:bg-neutral-800 ' +
    'px-2.5 py-1.5 text-sm text-stone-800 dark:text-neutral-100 ' +
    'focus:outline-none focus:ring-2 focus:ring-primary-500/40 focus:border-primary-500';

  // No profiles registered → show a hint, nothing to pick.
  if (!loading && profiles.length === 0) {
    return (
      <div className="text-xs text-neutral-500 dark:text-neutral-400">
        {t('claudeProfiles.picker.none', 'No Claude settings profiles yet — add one in Settings.')}
      </div>
    );
  }

  const fallbackOn = !!value.fallbackDirection;

  return (
    <div className="flex flex-col gap-2" style={selectStyle}>
      <div className="flex flex-wrap gap-2">
        <select
          className={selectClass}
          value={value.settingsProfile ?? ''}
          onChange={e => {
            const id = e.target.value || undefined;
            // Reset model + fallback when the profile changes.
            onChange({ settingsProfile: id });
          }}>
          <option value="">{t('claudeProfiles.picker.default', 'Default (no profile)')}</option>
          {profiles.map(p => (
            <option key={p.profile.id} value={p.profile.id}>
              {p.profile.name}
              {p.readable ? '' : ' ⚠'}
            </option>
          ))}
        </select>

        {selected && tiers.length > 0 && (
          <select
            className={selectClass}
            value={value.model ?? ''}
            onChange={e => onChange({ ...value, model: e.target.value || undefined })}>
            <option value="">{t('claudeProfiles.picker.modelDefault', 'Default model')}</option>
            {tiers.map(tier => (
              <option key={tier} value={tier}>
                {tierLabel(tier, selected.models)}
              </option>
            ))}
          </select>
        )}
      </div>

      {/* Fallback controls — only when a start profile+model is chosen and a
          ladder exists. */}
      {showFallback && selected && value.model && ladder.length > 0 && (
        <div className="flex flex-wrap items-center gap-2">
          <label className="flex items-center gap-1.5 text-xs text-stone-600 dark:text-neutral-300">
            <input
              type="checkbox"
              checked={fallbackOn}
              onChange={e =>
                onChange({
                  ...value,
                  fallbackDirection: e.target.checked ? 'down' : undefined,
                  fallbackEnd: undefined,
                })
              }
            />
            {t('claudeProfiles.picker.fallback', 'Fallback on startup failure')}
          </label>
          {fallbackOn && (
            <>
              <select
                className={selectClass}
                value={value.fallbackDirection ?? 'down'}
                onChange={e => onChange({ ...value, fallbackDirection: e.target.value })}>
                <option value="down">{t('claudeProfiles.picker.down', 'Step down ladder')}</option>
                <option value="up">{t('claudeProfiles.picker.up', 'Step up ladder')}</option>
              </select>
              <select
                className={selectClass}
                value={value.fallbackEnd ?? ''}
                onChange={e => onChange({ ...value, fallbackEnd: e.target.value || undefined })}>
                <option value="">
                  {t('claudeProfiles.picker.endBoundary', 'End: ladder boundary')}
                </option>
                {ladder.map(s => (
                  <option
                    key={`${s.profile_id}:${s.tier}`}
                    value={encodeStep(s.profile_id, s.tier)}>
                    {t('claudeProfiles.picker.endPrefix', 'End at')} {s.profile_name} / {s.tier}
                  </option>
                ))}
              </select>
            </>
          )}
        </div>
      )}
    </div>
  );
}
