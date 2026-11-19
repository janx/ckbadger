import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
import { http, HttpResponse } from 'msw';
import { server } from '../msw/server';
import { FiberChannelStep } from '@/components/fiber/fiber-channel-step';
import { scriptLookupKey } from '@/lib/script-pair';
import fixture from '../fixtures/fiber-channel-c07e.json';
import { canvasLines, seamLines } from '@/__tests__/utils/tx-lines';

vi.mock('@/components/layout/header', () => ({ Header: () => null }));

// The API's names, by pair (lane rd/api-lock-names, T N7): each of these locks by `type`; the
// same code hash by another hash type is another script, which the lookup does not name.
const NAMES: Record<string, string> = {
  [scriptLookupKey('0xe45b1f8f21bff23137035a3ab751d75b36a981deec3e7820194b9c042967f4f1', 'type')]:
    'Fiber Funding Lock',
  [scriptLookupKey('0x2d45c4d3ed3e942f1945386ee82a5d1b7e4bb16d7fe1ab015421174ab747406c', 'type')]:
    'Fiber Commitment Lock',
  [scriptLookupKey('0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8', 'type')]:
    'Default Lock',
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
      // asked by pairs (lane rd/api-lock-names), answered by pair
      const { scripts } = (await request.json()) as {
        scripts: { codeHash: string; hashType: string }[];
      };
      return HttpResponse.json(
        Object.fromEntries(
          scripts.map(({ codeHash, hashType }) => [
            scriptLookupKey(codeHash, hashType),
            { codeHash, name: NAMES[scriptLookupKey(codeHash, hashType)] ?? 'Unknown' },
          ])
        )
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
        box={{ head: '→ step 3, after 6\u00a0epochs', lines: ['the commitment lock waits'] }}
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
    // C11 round 3: across the seam, only that link (the funding cell continuing as the
    // commitment); the bridge carries the channel on past the sheet
    const links = seamLines(container);
    expect(links).toHaveLength(1);
    expect(links[0]).toHaveAttribute('data-continuity-kind', 'file');
    const bridge = Array.from(container.querySelectorAll('[data-onward-bridge] path'));
    expect(bridge).toHaveLength(1);
    expect(canvasLines(container)).toEqual([...links, ...bridge]);
    // C11: the step's two sheets end their heads with the API's exact totals (inputsCapacity,
    // outputsCapacity), and the fee closes the created sheet; round 2: the consumed total is no
    // longer repeated as a "Capacity entering the pool" line under the sheets
    const consumed = container.querySelector('[data-sheet="consumed"]') as HTMLElement;
    const created = container.querySelector('[data-sheet="created"]') as HTMLElement;
    expect(within(consumed).getByRole('heading', { name: '1 cell consumed' })).toBeInTheDocument();
    expect(consumed.querySelector('[data-sheet-total]')).toHaveTextContent(/^1,062(\.0+)? CKB$/);
    expect(created.querySelector('[data-sheet-total]')).toHaveTextContent(/^1,061\.99999545 CKB$/);
    expect(within(created).getByTestId('tx-fee')).toHaveTextContent(/^fee0\.00000455 CKB$/);
    expect(screen.queryByText(/entering the pool/i)).toBeNull();
    // the step's box: each line as wide as its words (R16: a text box is its words)
    expect(screen.getByText('→ step 3, after 6 epochs').parentElement).toHaveClass('items-start');
    // the step's one caption, at caption width (R14)
    expect(screen.getByText('One side posted the commitment.')).toHaveClass(
      'text-body',
      // re-pinned with readability R4 (lane B7): on the measure, 76 characters (was max-w-[640px])
      'max-w-measure'
    );
    // script groups stay out of tier 1 (FF-C5)
    expect(screen.queryByText('Script groups')).toBeNull();
    // Re-pinned on purpose (lane P4, the link-look gate: a link states its look through
    // components/ui/link.tsx and writes no ink by hand): the step's transaction is its key, the
    // bright ink of the `key` look (it was the link cyan through --ink, by hand), its block a link
    // inside the header's line, and the header's values read in the reading ink (they were quiet);
    // the box's dashed edge stays in the quiet slate; no bracket-hex class
    const tx = within(step).getByRole('link', { name: /^0x/ });
    expect(tx).toHaveClass('text-text-bright');
    expect(tx.style.getPropertyValue('--ink')).toBe('');
    expect(tx.style.color).toBe('');
    expect(tx.parentElement).toHaveClass('text-text', 'text-small');
    expect(tx.parentElement).not.toHaveClass('text-text-dim');
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
