/**
 * SAP Connections — central hub for SAP system integrations.
 *
 * Mirrors the layout of the Connections page (TwoPaneNav sidebar +
 * PanelPage content area) with tabs tailored to SAP use-cases.
 */
import { useMemo } from 'react';
import { useLocation, useNavigate } from 'react-router-dom';

import PanelPage from '../components/layout/PanelPage';
import { SidebarContent } from '../components/layout/shell/SidebarSlot';
import TwoPaneNav from '../components/layout/TwoPaneNav';
import { useT } from '../lib/i18n/I18nContext';

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
  return (
    <EmptyState
      title={t('sap.tabs.systems.emptyTitle')}
      description={t('sap.tabs.systems.emptyDesc')}
    />
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
