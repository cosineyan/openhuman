/**
 * SAP Connections — central hub for SAP system integrations.
 *
 * Mirrors the layout of the Connections page (TwoPaneNav sidebar +
 * PanelPage content area) with tabs tailored to SAP use-cases.
 */
import { useCallback, useEffect, useMemo, useState } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import PanelPage from '../components/layout/PanelPage';
import { SidebarContent } from '../components/layout/shell/SidebarSlot';
import TwoPaneNav from '../components/layout/TwoPaneNav';
import { useT } from '../lib/i18n/I18nContext';
import {
  getM365TokenStatus,
  getMcpChromeStatus,
  m365AuthLogin,
  m365AuthLogout,
  m365AuthRefresh,
  type M365TokenStatus,
  type MpcChromeStatus,
} from '../services/api/m365Api';

// ─────────────────────────────────────────────────────────────────────────────
// Types
// ─────────────────────────────────────────────────────────────────────────────

type SapTab = 'systems' | 'credentials' | 'modules' | 'skills';

const DEFAULT_TAB: SapTab = 'systems';

// ─────────────────────────────────────────────────────────────────────────────
// Helpers
// ─────────────────────────────────────────────────────────────────────────────

function navIcon(d: string) {
  return (
    <svg
      className="h-4 w-4 shrink-0"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.8}
      viewBox="0 0 24 24">
      <path strokeLinecap="round" strokeLinejoin="round" d={d} />
    </svg>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Empty-state placeholder shown until real content is built out
// ─────────────────────────────────────────────────────────────────────────────

function EmptyState({ title, description }: { title: string; description: string }) {
  return (
    <div className="flex h-full flex-col items-center justify-center gap-3 text-center">
      <div className="rounded-full bg-stone-100 p-4 dark:bg-neutral-800">
        <svg
          className="h-8 w-8 text-stone-400 dark:text-neutral-500"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          viewBox="0 0 24 24">
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M13.5 16.875h3.375m0 0h3.375m-3.375 0V13.5m0 3.375v3.375M6 10.5h2.25a2.25 2.25 0 002.25-2.25V6a2.25 2.25 0 00-2.25-2.25H6A2.25 2.25 0 003.75 6v2.25A2.25 2.25 0 006 10.5zm0 9.75h2.25A2.25 2.25 0 0010.5 18v-2.25a2.25 2.25 0 00-2.25-2.25H6a2.25 2.25 0 00-2.25 2.25V18A2.25 2.25 0 006 20.25zm9.75-9.75H18a2.25 2.25 0 002.25-2.25V6A2.25 2.25 0 0018 3.75h-2.25A2.25 2.25 0 0013.5 6v2.25a2.25 2.25 0 002.25 2.25z"
          />
        </svg>
      </div>
      <div>
        <p className="text-sm font-medium text-stone-700 dark:text-neutral-200">{title}</p>
        <p className="mt-1 max-w-xs text-xs text-stone-500 dark:text-neutral-400">{description}</p>
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Tab panels
// ─────────────────────────────────────────────────────────────────────────────
// Tile status helpers
// ─────────────────────────────────────────────────────────────────────────────

type TileState = 'connected' | 'expired' | 'disconnected' | 'loading';

function tileClasses(state: TileState) {
  switch (state) {
    case 'connected':
      return 'border-sage-300 bg-sage-50/80 shadow-[0_0_0_1px_rgba(34,197,94,0.12)] dark:border-sage-500/30 dark:bg-sage-500/10';
    case 'expired':
      return 'border-amber-200 bg-amber-50/40 dark:border-amber-500/30 dark:bg-amber-500/10';
    case 'disconnected':
    default:
      return 'border-stone-200 bg-white hover:bg-stone-50 dark:border-neutral-800 dark:bg-neutral-900 dark:hover:bg-neutral-800/60';
  }
}

function tileLabelClasses(state: TileState) {
  switch (state) {
    case 'connected':
      return 'text-sage-600 dark:text-sage-300';
    case 'expired':
      return 'text-amber-600 dark:text-amber-300';
    default:
      return 'text-stone-400 dark:text-neutral-500';
  }
}

// ─────────────────────────────────────────────────────────────────────────────

function SystemsTab() {
  const { t } = useT();
  const [chromeStatus, setChromeStatus] = useState<MpcChromeStatus | null>(null);
  const [status, setStatus] = useState<M365TokenStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<'login' | 'refresh' | 'logout' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [chrome, tokens] = await Promise.all([getMcpChromeStatus(), getM365TokenStatus()]);
      setChromeStatus(chrome);
      setStatus(tokens);
      setError(null);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void load();
    const id = setInterval(() => void load(), 60_000);
    return () => clearInterval(id);
  }, [load]);

  const handleLogin = async () => {
    setBusy('login');
    setError(null);
    try {
      const s = await m365AuthLogin();
      setStatus(s);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const handleRefresh = async () => {
    setBusy('refresh');
    setError(null);
    try {
      const s = await m365AuthRefresh();
      setStatus(s);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const handleLogout = async () => {
    setBusy('logout');
    setError(null);
    try {
      await m365AuthLogout();
      setStatus(null);
      await load();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  };

  const isConnected = status?.graph?.valid || status?.rest?.valid || status?.teams?.valid;

  const chromeState: TileState = !chromeStatus
    ? 'loading'
    : chromeStatus.ok
      ? 'connected'
      : 'disconnected';

  const tokenState = (entry: { valid: boolean; cached: boolean } | undefined): TileState => {
    if (!entry) return 'disconnected';
    if (entry.valid) return 'connected';
    if (entry.cached) return 'expired';
    return 'disconnected';
  };

  const tiles = [
    {
      key: 'chrome',
      label: t('sap.systems.mcpChrome.title'),
      sublabel: chromeStatus?.ok ? `:${chromeStatus.port}` : null,
      state: chromeState,
      icon: (
        <svg
          className="h-8 w-8"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          viewBox="0 0 24 24">
          <circle cx="12" cy="12" r="9" />
          <circle cx="12" cy="12" r="3.5" />
          <line x1="12" y1="2.5" x2="12" y2="8.5" />
          <line x1="20.5" y1="16.5" x2="15.5" y2="13.5" />
          <line x1="3.5" y1="16.5" x2="8.5" y2="13.5" />
        </svg>
      ),
    },
    {
      key: 'rest',
      label: t('sap.systems.rest'),
      sublabel:
        status?.rest?.expiresInMin != null && status.rest.valid
          ? `${status.rest.expiresInMin}m`
          : null,
      state: tokenState(status?.rest),
      icon: (
        <svg className="h-8 w-8" viewBox="0 0 24 24" fill="currentColor">
          <path d="M0 0h12v12H0zm12 12h12v12H12zM12 0h12v12H12zM0 12h12v12H0z" />
        </svg>
      ),
    },
    {
      key: 'graph',
      label: t('sap.systems.graph'),
      sublabel:
        status?.graph?.expiresInMin != null && status.graph.valid
          ? `${status.graph.expiresInMin}m`
          : null,
      state: tokenState(status?.graph),
      icon: (
        <svg
          className="h-8 w-8"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          viewBox="0 0 24 24">
          <path strokeLinecap="round" strokeLinejoin="round" d="M12 4.5v15m7.5-7.5h-15" />
          <circle cx="12" cy="12" r="9" />
        </svg>
      ),
    },
    {
      key: 'teams',
      label: t('sap.systems.teams'),
      sublabel:
        status?.teams?.expiresInMin != null && status.teams.valid
          ? `${status.teams.expiresInMin}m`
          : null,
      state: tokenState(status?.teams),
      icon: (
        <svg
          className="h-8 w-8"
          fill="none"
          stroke="currentColor"
          strokeWidth={1.5}
          viewBox="0 0 24 24">
          <path
            strokeLinecap="round"
            strokeLinejoin="round"
            d="M18 18.72a9.094 9.094 0 003.741-.479 3 3 0 00-4.682-2.72m.94 3.198l.001.031c0 .225-.012.447-.037.666A11.944 11.944 0 0112 21c-2.17 0-4.207-.576-5.963-1.584A6.062 6.062 0 016 18.719m12 0a5.971 5.971 0 00-.941-3.197m0 0A5.995 5.995 0 0012 12.75a5.995 5.995 0 00-5.058 2.772m0 0a3 3 0 00-4.681 2.72 8.986 8.986 0 003.74.477m.94-3.197a5.971 5.971 0 00-.94 3.197M15 6.75a3 3 0 11-6 0 3 3 0 016 0zm6 3a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0zm-13.5 0a2.25 2.25 0 11-4.5 0 2.25 2.25 0 014.5 0z"
          />
        </svg>
      ),
    },
  ] as const;

  return (
    <div className="space-y-4">
      {/* Icon grid */}
      <div
        className="grid gap-2 sm:gap-3"
        style={{
          gridTemplateColumns: 'repeat(auto-fill, minmax(5.5rem, 1fr))',
          gridAutoRows: '7rem',
        }}>
        {loading
          ? tiles.map(tile => (
              <div
                key={tile.key}
                className="rounded-2xl border border-stone-200 bg-stone-50 dark:border-neutral-800 dark:bg-neutral-900 animate-pulse"
              />
            ))
          : tiles.map(tile => (
              <div
                key={tile.key}
                className={`relative flex flex-col items-center justify-center rounded-2xl border p-3 text-center transition-colors ${tileClasses(tile.state)}`}>
                <div className="flex h-12 w-12 flex-shrink-0 items-center justify-center text-stone-600 dark:text-neutral-300 [&_svg]:h-8 [&_svg]:w-8">
                  {tile.icon}
                </div>
                <span className="mt-1.5 line-clamp-1 w-full text-[10px] font-medium text-stone-600 dark:text-neutral-300 leading-tight">
                  {tile.label}
                </span>
                <span className={`text-[9px] font-medium ${tileLabelClasses(tile.state)}`}>
                  {tile.state === 'connected'
                    ? (tile.sublabel ?? t('sap.systems.valid'))
                    : tile.state === 'expired'
                      ? t('sap.systems.expired')
                      : t('sap.systems.notCached')}
                </span>
              </div>
            ))}
      </div>

      {/* Action buttons */}
      <div className="flex items-center gap-2">
        {isConnected ? (
          <>
            <button
              type="button"
              onClick={() => void handleRefresh()}
              disabled={!!busy}
              className="text-xs px-3 py-1.5 rounded-md border border-stone-200 dark:border-neutral-700 text-stone-600 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800 disabled:opacity-50 transition-colors font-medium">
              {busy === 'refresh' ? t('sap.systems.refreshing') : t('sap.systems.refresh')}
            </button>
            <button
              type="button"
              onClick={() => void handleLogout()}
              disabled={!!busy}
              className="text-xs px-3 py-1.5 rounded-md border border-red-200 dark:border-red-800 text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/40 disabled:opacity-50 transition-colors font-medium">
              {busy === 'logout' ? t('sap.systems.disconnecting') : t('sap.systems.disconnect')}
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => void handleLogin()}
            disabled={!!busy}
            className="text-xs px-3 py-1.5 rounded-md bg-primary-500 text-white hover:bg-primary-600 disabled:opacity-50 transition-colors font-medium">
            {busy === 'login' ? t('sap.systems.connecting') : t('sap.systems.connect')}
          </button>
        )}
      </div>

      {/* mcp-chrome install hint */}
      {chromeStatus && !chromeStatus.ok && (
        <div className="rounded-lg border border-amber-200 dark:border-amber-800/50 bg-amber-50 dark:bg-amber-900/20 px-3 py-2.5">
          <p className="text-xs text-amber-700 dark:text-amber-300">
            {t('sap.systems.mcpChrome.hint')}
          </p>
          <a
            href="https://chromewebstore.google.com/detail/mcp-chrome/igncpeomfkelgijlakkcblpjhcmlhflo"
            target="_blank"
            rel="noreferrer"
            className="mt-1 inline-block text-xs text-primary-500 hover:text-primary-600 hover:underline">
            {t('sap.systems.mcpChrome.installLink')} →
          </a>
        </div>
      )}

      {error && <p className="text-xs text-red-600 dark:text-red-400">{error}</p>}

      <p className="text-xs text-stone-400 dark:text-neutral-500">{t('sap.systems.hint')}</p>
    </div>
  );
}

function CredentialsTab() {
  const { t } = useT();
  return (
    <EmptyState
      title={t('sap.tabs.credentials.emptyTitle')}
      description={t('sap.tabs.credentials.emptyDesc')}
    />
  );
}

function ModulesTab() {
  const { t } = useT();
  return (
    <EmptyState
      title={t('sap.tabs.modules.emptyTitle')}
      description={t('sap.tabs.modules.emptyDesc')}
    />
  );
}

function SkillsTab() {
  const { t } = useT();
  return (
    <EmptyState
      title={t('sap.tabs.skills.emptyTitle')}
      description={t('sap.tabs.skills.emptyDesc')}
    />
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Main page
// ─────────────────────────────────────────────────────────────────────────────

export default function SapConnectionsPage() {
  const { t } = useT();
  const location = useLocation();
  const navigate = useNavigate();

  const activeTab = useMemo<SapTab>(() => {
    const raw = new URLSearchParams(location.search).get('tab');
    if (raw === 'systems' || raw === 'credentials' || raw === 'modules' || raw === 'skills') {
      return raw;
    }
    return DEFAULT_TAB;
  }, [location.search]);

  const handleTabChange = (tab: SapTab) => {
    navigate(`/sap-connections?tab=${tab}`, { replace: true });
  };

  return (
    <div className="h-full">
      <SidebarContent>
        <div className="h-full overflow-hidden">
          <TwoPaneNav
            ariaLabel={t('nav.sapConnections')}
            selected={activeTab}
            onSelect={value => handleTabChange(value as SapTab)}
            header={
              <div className="px-1 pb-2 pt-1">
                <h2 className="text-sm font-semibold text-stone-800 dark:text-neutral-100">
                  {t('nav.sapConnections')}
                </h2>
                <p className="mt-0.5 text-xs text-stone-500 dark:text-neutral-400">
                  {t('sap.subtitle')}
                </p>
              </div>
            }
            groups={[
              {
                label: t('sap.groups.setup'),
                items: [
                  {
                    value: 'systems',
                    label: t('sap.tabs.systems.label'),
                    icon: navIcon(
                      'M5 12h14M5 12a2 2 0 01-2-2V6a2 2 0 012-2h14a2 2 0 012 2v4a2 2 0 01-2 2M5 12a2 2 0 00-2 2v4a2 2 0 002 2h14a2 2 0 002-2v-4a2 2 0 00-2-2m-2-4h.01M17 16h.01'
                    ),
                  },
                  {
                    value: 'credentials',
                    label: t('sap.tabs.credentials.label'),
                    icon: navIcon(
                      'M15 7a2 2 0 012 2m4 0a6 6 0 01-7.743 5.743L11 17H9v2H7v2H4a1 1 0 01-1-1v-2.586a1 1 0 01.293-.707l5.964-5.964A6 6 0 1121 9z'
                    ),
                  },
                ],
              },
              {
                label: t('sap.groups.capabilities'),
                items: [
                  {
                    value: 'modules',
                    label: t('sap.tabs.modules.label'),
                    icon: navIcon(
                      'M4 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2V6zM14 6a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2V6zM4 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2H6a2 2 0 01-2-2v-2zM14 16a2 2 0 012-2h2a2 2 0 012 2v2a2 2 0 01-2 2h-2a2 2 0 01-2-2v-2z'
                    ),
                  },
                  {
                    value: 'skills',
                    label: t('sap.tabs.skills.label'),
                    icon: navIcon(
                      'M14.752 11.168l-3.197-2.132A1 1 0 0010 9.87v4.263a1 1 0 001.555.832l3.197-2.132a1 1 0 000-1.664zM21 12a9 9 0 11-18 0 9 9 0 0118 0z'
                    ),
                  },
                ],
              },
            ]}
          />
        </div>
      </SidebarContent>

      <PanelPage contentClassName="p-4">
        {activeTab === 'systems' && <SystemsTab />}
        {activeTab === 'credentials' && <CredentialsTab />}
        {activeTab === 'modules' && <ModulesTab />}
        {activeTab === 'skills' && <SkillsTab />}
      </PanelPage>
    </div>
  );
}
