/**
 * SubscriptionPlans — unit tests for the subscription plan selector.
 *
 * NOTE: useT is NOT mocked here — default I18nContext (en.ts) is active.
 * All text assertions use actual English strings from en.ts.
 */
import { render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import type { PlanTier } from '../../../../../types/api';
import SubscriptionPlans from '../SubscriptionPlans';

// ── Helpers ───────────────────────────────────────────────────────────────────

const defaultProps = {
  currentTier: 'FREE' as PlanTier,
  billingInterval: 'monthly' as const,
  setBillingInterval: vi.fn(),
  isPurchasing: false,
  purchasingTier: null,
  paymentConfirmed: false,
  onUpgrade: vi.fn(),
};

// ── Tests ─────────────────────────────────────────────────────────────────────

describe('SubscriptionPlans', () => {
  it('renders the monthly/annual billing interval buttons', () => {
    render(<SubscriptionPlans {...defaultProps} />);
    // en.ts: 'settings.billing.subscription.monthly': 'Monthly'
    // en.ts: 'settings.billing.subscription.annual': 'Annual'
    expect(screen.getByText('Monthly')).toBeInTheDocument();
    expect(screen.getByText('Annual')).toBeInTheDocument();
  });

  it('shows payment-confirmed banner when paymentConfirmed=true', () => {
    render(<SubscriptionPlans {...defaultProps} paymentConfirmed={true} />);
    // en.ts: 'settings.billing.subscription.paymentConfirmed': 'Payment confirmed'
    expect(screen.getByText('Payment confirmed')).toBeInTheDocument();
  });

  it('shows waiting-for-payment banner when isPurchasing=true', () => {
    render(
      <SubscriptionPlans {...defaultProps} isPurchasing={true} purchasingTier={'PRO' as PlanTier} />
    );
    // en.ts: 'settings.billing.subscription.waitingPayment': 'Waiting payment'
    expect(screen.getByText('Waiting payment')).toBeInTheDocument();
  });
});
