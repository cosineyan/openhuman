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
  getMcpChromeStatus,
  getM365TokenStatus,
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

function SystemsTab() {
  const { t } = useT();
  const [chromeStatus, setChromeStatus] = useState<MpcChromeStatus | null>(null);
  const [status, setStatus] = useState<M365TokenStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<'login' | 'refresh' | 'logout' | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(async () => {
    try {
      const [chrome, tokens] = await Promise.all([
        getMcpChromeStatus(),
        getM365TokenStatus(),
      ]);
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

  return (
    <div className="space-y-4">
      {/* mcp-chrome extension status */}
      <div className="rounded-xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3">
          <div className="flex items-center gap-2">
            <svg className="h-4 w-4 shrink-0 text-stone-500" fill="none" stroke="currentColor" strokeWidth={1.8} viewBox="0 0 24 24">
              <path strokeLinecap="round" strokeLinejoin="round" d="M14 6l-1-2H5v17h2v-7h5l1 2h7V6h-6zm4 8h-4l-1-2H7V6h5l1 2h5v6z" />
            </svg>
            <span className="text-xs font-semibold text-stone-700 dark:text-neutral-200">
              {t('sap.systems.mcpChrome.title')}
            </span>
          </div>
          {chromeStatus ? (
            chromeStatus.ok ? (
              <span className="inline-flex items-center gap-1 text-xs text-green-600 dark:text-green-400">
                <span className="h-1.5 w-1.5 rounded-full bg-green-500" />
                {t('sap.systems.mcpChrome.connected')} :{chromeStatus.port}
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
                <span className="h-1.5 w-1.5 rounded-full bg-amber-500" />
                {t('sap.systems.mcpChrome.notFound')}
              </span>
            )
          ) : (
            <span className="text-xs text-stone-400 dark:text-neutral-500">{t('common.loading')}</span>
          )}
        </div>
        {chromeStatus && !chromeStatus.ok && (
          <div className="px-4 pb-3 border-t border-stone-100 dark:border-neutral-800">
            <p className="text-xs text-stone-500 dark:text-neutral-400 mt-2">
              {t('sap.systems.mcpChrome.hint')}
            </p>
            <a
              href="https://chromewebstore.google.com/detail/mcp-chrome/igncpeomfkelgijlakkcblpjhcmlhflo"
              target="_blank"
              rel="noreferrer"
              className="inline-block mt-2 text-xs text-primary-500 hover:text-primary-600 hover:underline">
              {t('sap.systems.mcpChrome.installLink')} →
            </a>
          </div>
        )}
      </div>

      {/* Microsoft 365 section */}
      <div className="rounded-xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3 border-b border-stone-100 dark:border-neutral-800">
          <div className="flex items-center gap-2">
            <svg className="h-5 w-5 text-blue-500 shrink-0" viewBox="0 0 24 24" fill="currentColor">
              <path d="M11.5 2.75h-8A.75.75 0 002.75 3.5v8c0 .414.336.75.75.75h8a.75.75 0 00.75-.75v-8a.75.75 0 00-.75-.75zM20.5 2.75h-5a.75.75 0 00-.75.75v5c0 .414.336.75.75.75h5a.75.75 0 00.75-.75v-5a.75.75 0 00-.75-.75zM20.5 12.5h-5a.75.75 0 00-.75.75v8c0 .414.336.75.75.75h5a.75.75 0 00.75-.75v-8a.75.75 0 00-.75-.75zM11.5 14.5h-8a.75.75 0 00-.75.75v5c0 .414.336.75.75.75h8a.75.75 0 00.75-.75v-5a.75.75 0 00-.75-.75z" />
            </svg>
            <span className="text-sm font-semibold text-stone-800 dark:text-neutral-100">
              {t('sap.systems.m365.title')}
            </span>
          </div>
          <div className="flex items-center gap-2">
            {isConnected ? (
              <>
                <button
                  type="button"
                  onClick={() => void handleRefresh()}
                  disabled={!!busy}
                  className="text-xs px-2.5 py-1 rounded-md border border-stone-200 dark:border-neutral-700 text-stone-600 dark:text-neutral-300 hover:bg-stone-50 dark:hover:bg-neutral-800 disabled:opacity-50 transition-colors">
                  {busy === 'refresh' ? t('sap.systems.refreshing') : t('sap.systems.refresh')}
                </button>
                <button
                  type="button"
                  onClick={() => void handleLogout()}
                  disabled={!!busy}
                  className="text-xs px-2.5 py-1 rounded-md border border-red-200 dark:border-red-800 text-red-600 dark:text-red-400 hover:bg-red-50 dark:hover:bg-red-950/40 disabled:opacity-50 transition-colors">
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
        </div>

        {loading ? (
          <div className="px-4 py-6 text-xs text-stone-400 dark:text-neutral-500 text-center">
            {t('common.loading')}
          </div>
        ) : (
          <div className="divide-y divide-stone-100 dark:divide-neutral-800">
            {(
              [
                { key: 'graph', label: t('sap.systems.graph') },
                { key: 'rest', label: t('sap.systems.rest') },
                { key: 'teams', label: t('sap.systems.teams') },
              ] as const
            ).map(({ key, label }) => {
              const entry = status?.[key];
              const valid = entry?.valid ?? false;
              const cached = entry?.cached ?? false;
              const mins = entry?.expiresInMin;
              return (
                <div key={key} className="flex items-center justify-between px-4 py-2.5">
                  <span className="text-xs text-stone-600 dark:text-neutral-300">{label}</span>
                  <div className="flex items-center gap-2">
                    {valid ? (
                      <>
                        <span className="inline-flex items-center gap-1 text-xs text-green-600 dark:text-green-400">
                          <span className="h-1.5 w-1.5 rounded-full bg-green-500" />
                          {t('sap.systems.valid')}
                          {mins !== null && mins !== undefined && ` (${mins}m)`}
                        </span>
                      </>
                    ) : cached ? (
                      <span className="inline-flex items-center gap-1 text-xs text-amber-600 dark:text-amber-400">
                        <span className="h-1.5 w-1.5 rounded-full bg-amber-500" />
                        {t('sap.systems.expired')}
                      </span>
                    ) : (
                      <span className="inline-flex items-center gap-1 text-xs text-stone-400 dark:text-neutral-500">
                        <span className="h-1.5 w-1.5 rounded-full bg-stone-300 dark:bg-neutral-600" />
                        {t('sap.systems.notCached')}
                      </span>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        )}
      </div>

      {error && <p className="text-xs text-red-600 dark:text-red-400 px-1">{error}</p>}

      <p className="text-xs text-stone-400 dark:text-neutral-500 px-1">{t('sap.systems.hint')}</p>
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
