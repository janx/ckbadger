import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ObjectCollectionStatCards } from '@/components/object/object-collection-stat-cards';
import { DotCellRecordsTable } from '@/components/identity/dotcell-records-table';
import { CapacityUtilization } from '@/components/ui/capacity-utilization';
import { CodeCellsList } from '@/components/ui/code-cells-list';
import { DataField, DataSection } from '@/components/ui/data-field';
import { HMultiplier } from '@/components/ui/h-multiplier';
import { Prose } from '@/components/ui/prose';
import { MiniStat, StatBlock } from '@/components/ui/stat-block';
import { NAVBAR_DROPDOWN_TRIGGER_CLASS } from '@/components/layout/navbar-dropdown-styles';
import { api } from '@/lib/api';

// Material pass v2, citypop edition — the per-page leftovers (review 2026-11-03): every remaining
// uppercase meta label is the shared `.eyebrow`; tier-2 explanations are set in one Prose piece;
// the chart detail page frames no chart; the script cells list's capacity is the shared exact
// figure, not gold; the Next-era shell that was never rendered is gone.

vi.mock('@/lib/api', () => ({
  api: { getCodeCells: vi.fn() },
  isWarmupPendingError: vi.fn(() => false),
  isNetworkInitializingError: vi.fn(() => false),
}));

const root = resolve(__dirname, '../..');
const css = readFileSync(join(root, 'app/globals.css'), 'utf-8');

function sourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) out.push(...sourceFiles(path));
    else if (/\.tsx?$/.test(entry)) out.push(path);
  }
  return out;
}

const eyebrow = (el: HTMLElement) => {
  expect(el).toHaveClass('eyebrow');
  expect(el.className).not.toMatch(/\b(uppercase|tracking-wide|tracking-wider|tracking-widest)\b/);
  expect(el.className).not.toMatch(/tracking-\[/);
};

describe('the eyebrow sweep: every uppercase meta label is the one .eyebrow', () => {
  it('leaves no ad-hoc uppercase or tracked label in app/ or components/', () => {
    const offenders: string[] = [];
    for (const file of [
      ...sourceFiles(join(root, 'app')),
      ...sourceFiles(join(root, 'components')),
    ]) {
      const lines = readFileSync(file, 'utf-8').split('\n');
      lines.forEach((line, i) => {
        // `normal-case` / `tracking-normal` reset a glyph inside an eyebrow (the pipeline's "?");
        // `tracking-tight` and a negative em are a figure's, not a label's.
        if (
          /\b(uppercase|tracking-wide|tracking-wider|tracking-widest)\b|tracking-\[0\./.test(line)
        )
          offenders.push(`${file.slice(root.length + 1)}:${i + 1}: ${line.trim()}`);
      });
    }
    expect(offenders).toEqual([]);
  });

  it('a data field’s vertical label and a data section’s title', () => {
    render(
      <DataSection title="Section title">
        <DataField label="Field label" layout="vertical">
          value
        </DataField>
      </DataSection>
    );
    eyebrow(screen.getByText('Section title'));
    expect(screen.getByText('Section title')).toHaveClass('text-small', 'text-text-dim');
    eyebrow(screen.getByText('Field label'));
  });

  it('a stat block’s label and a mini stat’s label', () => {
    render(
      <>
        <StatBlock label="Block label" value={12} />
        <MiniStat label="Mini label" value="1.2" />
      </>
    );
    eyebrow(screen.getByText('Block label'));
    eyebrow(screen.getByText('Mini label'));
    expect(screen.getByText('Mini label')).toHaveClass('text-small', 'text-text-dim');
  });

  it('the capacity-utilization and H-multiplier bar titles', () => {
    render(
      <>
        <CapacityUtilization
          subject="test"
          totalCapacity="100000000000"
          commonKnowledgeSize="50000000000"
          totalLabel="Utilization total"
        />
        <HMultiplier
          totalCapacity="100000000000"
          commonKnowledgeSize="50000000000"
          totalLabel="Multiplier total"
        />
      </>
    );
    eyebrow(screen.getByText('Utilization total'));
    eyebrow(screen.getByText('Multiplier total'));
  });

  it('a collection stat card’s label and a .cell records table head', () => {
    render(
      <>
        <ObjectCollectionStatCards totalCount={3} liveCount={2} createdAtBlock={10} />
        <DotCellRecordsTable
          records={[
            {
              key: 'k',
              label: 'l',
              valueHex: '0x76',
              valueUtf8: 'v',
              ttl: 1,
              decodedAddress: null,
            },
          ]}
        />
      </>
    );
    for (const label of ['Total Objects', 'Live Items', 'Created At'])
      eyebrow(screen.getByText(label));
    const head = screen.getByText('Key').closest('tr') as HTMLElement;
    eyebrow(head);
    expect(head).toHaveClass('text-small', 'text-text-dim');
  });

  it('the header dropdown trigger', () => {
    expect(NAVBAR_DROPDOWN_TRIGGER_CLASS).toMatch(/\beyebrow\b/);
    expect(NAVBAR_DROPDOWN_TRIGGER_CLASS).not.toMatch(/uppercase|tracking-/);
  });
});

describe('tier-2 prose: one piece, components/ui/prose.tsx', () => {
  it('globals.css sets a Prose paragraph at 14 px / 1.7 / 64ch, dim, 14 px apart, em italic', () => {
    const p = css.match(/\.prose-tier2 p\s*\{([^}]*)\}/)!;
    expect(p[1]).toContain('font-size: 14px');
    expect(p[1]).toContain('line-height: 1.7');
    expect(p[1]).toContain('max-width: 64ch');
    expect(p[1]).toContain('color: #7c849c');
    expect(css).toMatch(/\.prose-tier2 p \+ p\s*\{\s*margin-top: 14px;\s*\}/);
    expect(css).toMatch(/\.prose-tier2 em\s*\{\s*font-style: italic;\s*\}/);
  });

  it('Prose renders its children in one .prose-tier2 block and keeps a caller’s classes', () => {
    render(
      <Prose className="mb-3" aria-label="an explanation">
        <p>First.</p>
        <p>
          Second, with <em>emphasis</em>.
        </p>
      </Prose>
    );
    const prose = screen.getByLabelText('an explanation');
    expect(prose).toHaveAttribute('data-prose');
    expect(prose).toHaveClass('prose-tier2', 'min-w-0', 'mb-3');
    expect(within(prose).getAllByRole('paragraph')).toHaveLength(2);
    expect(within(prose).getByText('emphasis').tagName).toBe('EM');
  });
});

describe('no card per chart, no gold capacity, no dead shell', () => {
  it('the chart detail page frames no chart; the dead ChartPage that did is gone with its note', () => {
    const detail = readFileSync(join(root, 'components/charts/chart-detail.tsx'), 'utf-8');
    expect(detail).not.toMatch(/TerminalPanel/);
    expect(detail).toMatch(/import \{ Prose \} from '@\/components\/ui\/prose'/);
    // ChartPage (components/charts/chart-page.tsx) wrapped the full-size chart in the instrument
    // frame; nothing rendered it. ChartCard was the card per chart; nothing rendered it either.
    expect(existsSync(join(root, 'components/charts/chart-page.tsx'))).toBe(false);
    expect(existsSync(join(root, 'components/charts/chart-calculation-note.tsx'))).toBe(false);
    expect(existsSync(join(root, 'components/ui/chart-card.tsx'))).toBe(false);
  });

  it('the Next-era root layout is gone; the SPA entry owns globals.css', () => {
    // app/layout.tsx was the shell the SPA never rendered (src/main.tsx → AppRoot → AppFrame).
    expect(existsSync(join(root, 'app/layout.tsx'))).toBe(false);
    expect(existsSync(join(root, 'app/providers.tsx'))).toBe(false);
    expect(existsSync(join(root, 'components/hero-stat-row.tsx'))).toBe(false);
    expect(readFileSync(join(root, 'src/main.tsx'), 'utf-8')).toContain(
      "import '@/app/globals.css'"
    );
    expect(readFileSync(join(root, 'src/app/root.tsx'), 'utf-8')).toContain(
      "import { Providers } from '@/src/app/providers'"
    );
  });

  it('the script cells list draws capacity as the shared exact figure, ink with a dim unit, never gold', async () => {
    vi.mocked(api.getCodeCells).mockResolvedValue({
      codeCells: [
        {
          txHash: `0x${'ab'.repeat(32)}`,
          outputIndex: 0,
          status: 'live',
          createdAtBlock: 1234,
          capacity: '96886' + '00000000',
        },
      ],
      liveCount: 1,
      totalCount: 1,
    });
    const { container } = render(
      <CodeCellsList codeHash={`0x${'cd'.repeat(32)}`} hashType="type" />
    );
    const amount = await screen.findByText('96,886');
    const figure = amount.closest('[data-ckb-amount]') as HTMLElement;
    expect(figure).toHaveTextContent('96,886.00000000 CKB');
    expect(figure).toHaveClass('text-text', 'tabular-nums');
    expect(within(figure).getByText(/CKB$/)).toHaveClass('text-text-dim', 'text-meta');
    // the capacity cell carries no gold (the out point's hex colouring beside it is its own)
    const cell = figure.parentElement as HTMLElement;
    expect(cell).toHaveClass('text-body', 'text-right');
    expect(cell.innerHTML).not.toMatch(/text-gold|#f2c55c/);
    expect(container.querySelector('.text-gold')).toBeNull();
    // the Capacity / TerminalNumber gold figure has no users left here
    expect(existsSync(join(root, 'components/ui/capacity.tsx'))).toBe(false);
    // the list's column head is an eyebrow
    eyebrow(screen.getByText('Outpoint').parentElement as HTMLElement);
  });
});
