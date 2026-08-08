import type { PlanTier } from '../../../../types/api';
import SubscriptionPlans from './SubscriptionPlans';

interface BillingPlansTabProps {
  billingInterval: 'monthly' | 'annual';
  currentTier: PlanTier;
  isPurchasing: boolean;
  onUpgrade: (tier: PlanTier) => void;
  paymentConfirmed: boolean;
  purchasingTier: PlanTier | null;
  setBillingInterval: (value: 'monthly' | 'annual') => void;
}

export default function BillingPlansTab({
  billingInterval,
  currentTier,
  isPurchasing,
  onUpgrade,
  paymentConfirmed,
  purchasingTier,
  setBillingInterval,
}: BillingPlansTabProps) {
  return (
    <SubscriptionPlans
      currentTier={currentTier}
      billingInterval={billingInterval}
      setBillingInterval={setBillingInterval}
      isPurchasing={isPurchasing}
      purchasingTier={purchasingTier}
      paymentConfirmed={paymentConfirmed}
      onUpgrade={onUpgrade}
    />
  );
}
