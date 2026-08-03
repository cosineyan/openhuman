import { describe, expect, it, vi } from 'vitest';

import { FALLBACK_DEFINITIONS } from '../../lib/channels/definitions';
import { renderWithProviders } from '../../test/test-utils';
import ChannelSetupModal from './ChannelSetupModal';

vi.mock('../../services/api/channelConnectionsApi', () => ({
  channelConnectionsApi: { connectChannel: vi.fn(), disconnectChannel: vi.fn() },
}));
vi.mock('../../utils/tauriCommands/core', () => ({ restartCoreProcess: vi.fn() }));

const larkDefinition = FALLBACK_DEFINITIONS.find(def => def.id === 'lark')!;

describe('<ChannelSetupModal /> header logo (issue #2854)', () => {
  it('renders the Lark / Feishu brand logo in the modal header', () => {
    renderWithProviders(<ChannelSetupModal definition={larkDefinition} onClose={vi.fn()} />);
    expect(document.querySelector('img[src="/lark.png"]')).not.toBeNull();
  });

  // Regression: the modal's channel switch had no `case 'lark'`, so it fell to
  // the default "config not available" branch and rendered zero input fields.
  it('renders the Lark credential input fields (not the config-unavailable fallback)', () => {
    const { getByPlaceholderText } = renderWithProviders(
      <ChannelSetupModal definition={larkDefinition} onClose={vi.fn()} />
    );
    expect(getByPlaceholderText('cli_xxxxxxxxxxxx')).toBeInTheDocument(); // app_id
    expect(getByPlaceholderText('Your Lark app secret')).toBeInTheDocument(); // app_secret
  });
});
