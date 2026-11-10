import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { FiberChannelStep } from '@/components/fiber/fiber-channel-step';
import fixture from '../fixtures/fiber-channel-c07e.json';

vi.mock('@/components/layout/header', () => ({ Header: () => null }));

const NAMES: Record<string, string> = {
  '0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1': 'Fiber Funding Lock',
  '0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c': 'Fiber Commitment Lock',
  '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8': 'Default Lock',
};
function serve() {
  server.use(
    http.get('/api/:network/v1/transactions/:hash/detail', ({ params }) =>
      HttpResponse.json(
        (fixture.transactions as Record<string, unknown>)[String(params.hash)] ?? null,
        {
          status: (fixture.transactions as Record<string, unknown>)[String(params.hash)]
            ? 200
            : 404,
        }
      )
    ),
    http.post('/api/:network/v1/scripts/lookup', async ({ request }) => {
      const { codeHashes } = (await request.json()) as { codeHashes: string[] };
      return HttpResponse.json(
        Object.fromEntries(codeHashes.map((h) => [h, { codeHash: h, name: NAMES[h] ?? 'unknown' }]))
      );
    })
  );
}
const { channel, evidence } = fixture;

describe('FiberChannelStep (one txviz per on-chain step, FF-C4)', () => {
  it('names the locks, gives the channel cells their role, and carries the channel onward', async () => {
    serve();
    const { container } = render(
      <FiberChannelStep
        n={2}
        title="Force close"
        txHash={channel.closeTxHash}
        blockNumber={channel.closeBlock}
        timestamp={channel.closeTimestamp}
        description="One side posted the commitment."
        epoch="11639+836/1434"
        consumes={{ txHash: channel.fundingTxHash, index: 0, role: 'Fiber funding' }}
        carries={{ index: evidence.currentCommitment.outputIndex, role: 'Fiber commitment' }}
        box={{ head: '→ step 3, after 6 epochs', lines: ['the commitment lock waits'] }}
      />
    );
    expect(await screen.findByText('Fiber Funding Lock')).toBeInTheDocument();
    expect(screen.getByText('Fiber Commitment Lock')).toBeInTheDocument();
    const step = screen.getByRole('heading', { name: /^2 · Force close/ });
    expect(step).toHaveTextContent('#15,634,497');
    expect(step).toHaveTextContent('2025-03-14 14:15:32 UTC+8');
    // R2b-b-16: the stage's epoch beside its block (FF-C4)
    expect(step).toHaveTextContent('epoch 11,639 · 836 / 1,434');
    const input = container.querySelector('[data-cell-id="in:0"]') as HTMLElement;
    expect(within(input).getByText('Fiber funding')).toBeInTheDocument();
    const output = container.querySelector('[data-cell-id="out:0"]') as HTMLElement;
    expect(within(output).getByText('Fiber commitment')).toBeInTheDocument();
    expect(container.querySelector('[data-onward-bridge]')).toHaveAttribute(
      'data-onward-from',
      'out:0'
    );
    expect(screen.getByText(/in:0 → out:0 · the channel continues/)).toBeInTheDocument();
    // the step's box: each line as wide as its words (R16: a text box is its words)
    expect(screen.getByText('→ step 3, after 6 epochs').parentElement).toHaveClass('items-start');
    // the step's one caption, at caption width (R14)
    expect(screen.getByText('One side posted the commitment.')).toHaveClass(
      'text-body',
      'max-w-[640px]'
    );
    // script groups stay out of tier 1 (FF-C5)
    expect(screen.queryByText('Script groups')).toBeNull();
    // Visual finish V11: the tx link in the link cyan through --ink (so `a:hover` jade still
    // wins), the box's dashed edge in the quiet slate; no bracket-hex class
    const tx = within(step).getByRole('link', { name: /^0x/ });
    expect(tx).toHaveClass('text-[color:var(--ink)]', 'hover:underline');
    expect(tx.style.getPropertyValue('--ink')).toBe('var(--c-board-link)');
    expect(tx.style.color).toBe('');
    expect(screen.getByText('→ step 3, after 6 epochs').parentElement).toHaveStyle({
      borderColor: 'var(--c-ink-quiet)',
    });
    expect(container.innerHTML).not.toMatch(/(text|bg|border)-\[#/);
  });

  it('fails loudly when the channel cell it follows is not an input of the step', async () => {
    serve();
    render(
      <FiberChannelStep
        n={3}
        title="Settle"
        txHash={channel.settlementTxHash}
        blockNumber={channel.settlementBlock}
        timestamp={channel.settlementTimestamp}
        description=""
        consumes={{ txHash: channel.fundingTxHash, index: 0, role: 'Fiber funding' }}
        box={{ head: 'Closed', lines: [] }}
      />
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      /does not spend the Fiber funding cell 0xbcba6519.*:0/
    );
  });
});
