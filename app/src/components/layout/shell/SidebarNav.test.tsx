import { screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import { renderWithProviders } from '../../../test/test-utils';
import SidebarNav from './SidebarNav';

// Analytics is fire-and-forget; stub it so the nav renders without a transport.
vi.mock('../../../services/analytics', () => ({ trackEvent: vi.fn() }));

/** The rendered button for a nav label (label text lives in a child span). */
function tabButton(label: string): HTMLButtonElement {
  return screen.getByRole('button', { name: new RegExp(label) }) as HTMLButtonElement;
}

describe('SidebarNav active matching', () => {
  it('keeps Chat active on any nested /chat route', () => {
    // /chat matches by prefix so deep links keep the tab highlighted.
    renderWithProviders(<SidebarNav />, { initialEntries: ['/chat/some-thread'] });

    expect(tabButton('Chat')).toHaveAttribute('aria-current', 'page');
  });

  it('does not mark Chat active on an unrelated route', () => {
    renderWithProviders(<SidebarNav />, { initialEntries: ['/brain'] });

    expect(tabButton('Chat')).not.toHaveAttribute('aria-current');
    expect(tabButton('Brain')).toHaveAttribute('aria-current', 'page');
  });
});
