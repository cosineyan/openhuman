/**
 * SAP Connections — central hub for SAP system integrations.
 *
 * Layout mirrors the Connections page: TwoPaneNav sidebar + PanelPage content.
 * Systems tab style mirrors the Channels tab in Skills.tsx.
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
type TileStatus = 'connected' | 'expired' | 'disconnected';

const DEFAULT_TAB: SapTab = 'systems';

// ─────────────────────────────────────────────────────────────────────────────
// Status helpers — exact copy of channelStatusColor pattern from Skills.tsx
// ─────────────────────────────────────────────────────────────────────────────

function tileStatusColor(s: TileStatus): string {
  switch (s) {
    case 'connected':
      return 'text-sage-600 dark:text-sage-300';
    case 'expired':
      return 'text-amber-600 dark:text-amber-300';
    default:
      return 'text-stone-400 dark:text-neutral-500';
  }
}

function tileBorder(s: TileStatus): string {
  switch (s) {
    case 'connected':
      return 'border-sage-300 bg-sage-50/80 shadow-[0_0_0_1px_rgba(34,197,94,0.12)] hover:bg-sage-50 dark:border-sage-500/30 dark:bg-sage-500/10 dark:hover:bg-sage-500/15';
    case 'expired':
      return 'border-amber-200 bg-amber-50/40 hover:bg-amber-50/70 dark:border-amber-500/30 dark:bg-amber-500/10 dark:hover:bg-amber-500/15';
    default:
      return 'border-stone-200 bg-white hover:bg-stone-50 dark:border-neutral-800 dark:bg-neutral-900 dark:hover:bg-neutral-800/60';
  }
}

function tokenTileStatus(entry: { valid: boolean; cached: boolean } | undefined): TileStatus {
  if (!entry) return 'disconnected';
  if (entry.valid) return 'connected';
  if (entry.cached) return 'expired';
  return 'disconnected';
}

// ─────────────────────────────────────────────────────────────────────────────
// ConnectionTile — mirrors ChannelTile from Skills.tsx exactly
// ─────────────────────────────────────────────────────────────────────────────

interface ConnectionTileProps {
  name: string;
  statusLabel: string;
  status: TileStatus;
  icon: React.ReactNode;
  onClick?: () => void;
}

function ConnectionTile({ name, statusLabel, status, icon, onClick }: ConnectionTileProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={`group flex flex-col items-center gap-2 rounded-2xl border p-3 pb-3 text-center transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-primary-500/40 ${tileBorder(status)}`}>
      {/* Exact same icon container as ChannelTile in Skills.tsx */}
      <div className="relative flex h-12 w-12 flex-shrink-0 items-center justify-center text-stone-700 dark:text-neutral-200 [&>span]:h-12 [&>span]:w-12 [&>span]:rounded-2xl [&_img]:max-h-10 [&_img]:max-w-10 [&_svg]:h-8 [&_svg]:w-8">
        {icon}
      </div>
      <div className="flex min-h-[2.5rem] w-full min-w-0 flex-col items-center justify-start gap-0.5">
        <span className="line-clamp-2 text-[11px] font-semibold leading-tight text-stone-900 dark:text-neutral-100">
          {name}
        </span>
        <span className={`line-clamp-1 text-[10px] font-medium ${tileStatusColor(status)}`}>
          {statusLabel}
        </span>
      </div>
    </button>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Service icon badges — same structure as ComposioLogoBadge in toolkitMeta.tsx:
// span h-8 w-8 rounded-xl bg-white shadow-sm ring-1 ring-black/5
// ChannelTile's [&>span]:h-12 [&>span]:w-12 auto-scales the span to 48px
// ─────────────────────────────────────────────────────────────────────────────

function LogoBadge({ children }: { children: React.ReactNode }) {
  return (
    <span className="flex h-8 w-8 items-center justify-center overflow-hidden rounded-xl bg-white dark:bg-neutral-900 shadow-sm ring-1 ring-black/5">
      {children}
    </span>
  );
}

// Outlook: uses logos.composio.dev (200 OK)
function OutlookLogoBadge() {
  const [failed, setFailed] = useState(false);
  if (failed)
    return (
      <LogoBadge>
        <svg viewBox="0 0 24 24" fill="#0078D4" className="h-6 w-6 p-0.5">
          <rect x="2" y="6" width="20" height="12" rx="2" />
          <path
            d="M2 8l10 6 10-6"
            stroke="white"
            strokeWidth="1.5"
            fill="none"
            strokeLinecap="round"
          />
        </svg>
      </LogoBadge>
    );
  return (
    <LogoBadge>
      <img
        src="https://logos.composio.dev/api/outlook"
        alt="Outlook"
        className="h-full w-full object-contain p-1"
        loading="lazy"
        onError={() => setFailed(true)}
      />
    </LogoBadge>
  );
}

// Graph API: Microsoft 4-square logo
function GraphLogoBadge() {
  return (
    <LogoBadge>
      <svg viewBox="0 0 24 24" fill="none" className="h-6 w-6 p-0.5">
        <rect x="2" y="2" width="9" height="9" rx="1" fill="#0078D4" />
        <rect x="13" y="2" width="9" height="9" rx="1" fill="#0078D4" opacity="0.75" />
        <rect x="2" y="13" width="9" height="9" rx="1" fill="#0078D4" opacity="0.75" />
        <rect x="13" y="13" width="9" height="9" rx="1" fill="#0078D4" opacity="0.45" />
      </svg>
    </LogoBadge>
  );
}

// Teams: purple T + people
function TeamsLogoBadge() {
  return (
    <LogoBadge>
      <svg viewBox="0 0 24 24" fill="#5059C9" className="h-6 w-6 p-0.5">
        <circle cx="14.5" cy="6.5" r="3" />
        <path d="M9 18c0-3.038 2.462-5.5 5.5-5.5S20 14.962 20 18H9z" />
        <circle cx="8" cy="8.5" r="2.2" />
        <path
          d="M3.5 18c0-2.485 2.015-4.5 4.5-4.5H10a4.5 4.5 0 011.3.19A5.5 5.5 0 009 18H3.5z"
          opacity="0.7"
        />
      </svg>
    </LogoBadge>
  );
}

// mcp-chrome: Chrome-inspired icon
function ChromeLogoBadge() {
  return (
    <LogoBadge>
      <svg viewBox="0 0 24 24" fill="none" className="h-6 w-6 p-0.5">
        <circle cx="12" cy="12" r="4.5" fill="#4F46E5" />
        <circle cx="12" cy="12" r="9" stroke="#4F46E5" strokeWidth="1.8" />
        <line
          x1="12"
          y1="3"
          x2="12"
          y2="7.5"
          stroke="#4F46E5"
          strokeWidth="1.8"
          strokeLinecap="round"
        />
        <line
          x1="20.2"
          y1="16.5"
          x2="16.3"
          y2="14.2"
          stroke="#4F46E5"
          strokeWidth="1.8"
          strokeLinecap="round"
        />
        <line
          x1="3.8"
          y1="16.5"
          x2="7.7"
          y2="14.2"
          stroke="#4F46E5"
          strokeWidth="1.8"
          strokeLinecap="round"
        />
      </svg>
    </LogoBadge>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Systems Tab
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

  const tiles = [
    {
      key: 'chrome',
      name: 'mcp-chrome',
      status: (!chromeStatus
        ? 'disconnected'
        : chromeStatus.ok
          ? 'connected'
          : 'disconnected') as TileStatus,
      sublabel: chromeStatus?.ok ? `:${chromeStatus.port}` : undefined,
      icon: <ChromeLogoBadge />,
    },
    {
      key: 'rest',
      name: 'Outlook',
      status: tokenTileStatus(status?.rest),
      sublabel:
        status?.rest?.expiresInMin != null && status.rest.valid
          ? `${status.rest.expiresInMin}m`
          : undefined,
      icon: <OutlookLogoBadge />,
    },
    {
      key: 'graph',
      name: 'Graph API',
      status: tokenTileStatus(status?.graph),
      sublabel:
        status?.graph?.expiresInMin != null && status.graph.valid
          ? `${status.graph.expiresInMin}m`
          : undefined,
      icon: <GraphLogoBadge />,
    },
    {
      key: 'teams',
      name: 'Teams',
      status: tokenTileStatus(status?.teams),
      sublabel:
        status?.teams?.expiresInMin != null && status.teams.valid
          ? `${status.teams.expiresInMin}m`
          : undefined,
      icon: <TeamsLogoBadge />,
    },
  ] as const;

  return (
    <div className="rounded-2xl border border-stone-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 p-3 shadow-soft animate-fade-up">
      {/* Header — mirrors channels tab header */}
      <div className="px-1 pb-3 pt-1">
        <h2 className="flex items-center gap-2 text-sm font-semibold text-stone-900 dark:text-neutral-100">
          <span className="inline-flex h-6 w-6 items-center justify-center rounded-full bg-stone-100 dark:bg-neutral-800">
            <svg
              className="h-3.5 w-3.5 text-stone-500 dark:text-neutral-400"
              fill="none"
              stroke="currentColor"
              strokeWidth={2}
              viewBox="0 0 24 24">
              <path
                strokeLinecap="round"
                strokeLinejoin="round"
                d="M8.288 15.038a5.25 5.25 0 017.424 0M5.106 11.856c3.807-3.808 9.98-3.808 13.788 0M1.924 8.674c5.565-5.565 14.587-5.565 20.152 0M12.53 18.22l-.53.53-.53-.53a.75.75 0 011.06 0z"
              />
            </svg>
          </span>
          {t('sap.systems.m365.title')}
        </h2>
        <p className="mt-0.5 text-[11px] leading-relaxed text-stone-500 dark:text-neutral-400">
          {t('sap.systems.hint')}
        </p>
      </div>

      {/* Icon grid — exact same layout as channels tab */}
      {loading ? (
        <div
          className="grid gap-2 sm:gap-3"
          style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(5.5rem, 1fr))' }}>
          {[0, 1, 2, 3].map(i => (
            <div
              key={i}
              className="h-[7rem] rounded-2xl border border-stone-100 dark:border-neutral-800 bg-stone-50 dark:bg-neutral-800/50 animate-pulse"
            />
          ))}
        </div>
      ) : (
        <div
          className="grid gap-2 sm:gap-3"
          style={{ gridTemplateColumns: 'repeat(auto-fill, minmax(5.5rem, 1fr))' }}>
          {tiles.map(tile => (
            <ConnectionTile
              key={tile.key}
              name={tile.name}
              status={tile.status}
              statusLabel={
                tile.status === 'connected'
                  ? (tile.sublabel ?? t('sap.systems.valid'))
                  : tile.status === 'expired'
                    ? t('sap.systems.expired')
                    : t('sap.systems.notCached')
              }
              icon={tile.icon}
            />
          ))}
        </div>
      )}

      {/* mcp-chrome install hint */}
      {!loading && chromeStatus && !chromeStatus.ok && (
        <div className="mt-3 rounded-xl border border-amber-200 dark:border-amber-800/40 bg-amber-50 dark:bg-amber-900/20 px-3 py-2.5">
          <p className="text-[11px] text-amber-700 dark:text-amber-300">
            {t('sap.systems.mcpChrome.hint')}
          </p>
          <a
            href="https://chromewebstore.google.com/detail/mcp-chrome/igncpeomfkelgijlakkcblpjhcmlhflo"
            target="_blank"
            rel="noreferrer"
            className="mt-1 inline-block text-[11px] text-primary-500 hover:text-primary-600 hover:underline">
            {t('sap.systems.mcpChrome.installLink')} →
          </a>
        </div>
      )}

      {/* Connect / Refresh / Disconnect — at the bottom like channel default selector */}
      <div className="mt-4 flex items-center gap-2 border-t border-stone-100 dark:border-neutral-800 pt-3">
        {isConnected ? (
          <>
            <button
              type="button"
              onClick={() => void handleRefresh()}
              disabled={!!busy}
              className="rounded-lg border border-stone-200 dark:border-neutral-700 bg-stone-50 dark:bg-neutral-800/60 px-3 py-1.5 text-xs font-medium text-stone-600 dark:text-neutral-300 hover:border-stone-300 dark:hover:border-neutral-600 disabled:opacity-50 transition-colors">
              {busy === 'refresh' ? t('sap.systems.refreshing') : t('sap.systems.refresh')}
            </button>
            <button
              type="button"
              onClick={() => void handleLogout()}
              disabled={!!busy}
              className="rounded-lg border border-red-200 dark:border-red-800/60 px-3 py-1.5 text-xs font-medium text-red-600 dark:text-red-400 hover:border-red-300 dark:hover:border-red-700 hover:bg-red-50 dark:hover:bg-red-950/30 disabled:opacity-50 transition-colors">
              {busy === 'logout' ? t('sap.systems.disconnecting') : t('sap.systems.disconnect')}
            </button>
          </>
        ) : (
          <button
            type="button"
            onClick={() => void handleLogin()}
            disabled={!!busy}
            className="rounded-lg bg-primary-500 px-4 py-1.5 text-xs font-semibold text-white hover:bg-primary-600 disabled:opacity-50 transition-colors">
            {busy === 'login' ? t('sap.systems.connecting') : t('sap.systems.connect')}
          </button>
        )}
        {error && <p className="text-[11px] text-red-600 dark:text-red-400 ml-1">{error}</p>}
      </div>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────────────────────
// Other tab placeholders
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
// Nav icon helper
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
// Main page
// ─────────────────────────────────────────────────────────────────────────────

export default function SapConnectionsPage() {
  const { t } = useT();
  const location = useLocation();
  const navigate = useNavigate();

  const activeTab = useMemo<SapTab>(() => {
    const raw = new URLSearchParams(location.search).get('tab');
    if (raw === 'systems' || raw === 'credentials' || raw === 'modules' || raw === 'skills')
      return raw;
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
