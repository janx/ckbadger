import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { BlockPosition } from '@/components/blocks/block-position';
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';
import { fullText } from '@/__tests__/utils/full-text';
import {
  allTextLines,
  expectApart,
  expectInside,
  rectBox,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';

const parent = `0x${'a'.repeat(64)}`;
const uncle = `0x${'b'.repeat(64)}`;
const block = {
  ...blockEvidence,
  number: 932,
  hash: `0x${'c'.repeat(64)}`,
  parentHash: `0x${'d'.repeat(64)}`,
  timestamp: '2026-10-26T08:55:37Z',
  confirmations: 3,
  proposalsCount: 1,
  unclesCount: 1,
  uncles: [
    {
      number: 931,
      hash: uncle,
      parentHash: parent,
      timestamp: '2026-10-26T08:55:26Z',
      proposalIds: [],
    },
  ],
};
const incoming = {
  hash: '0xtx1',
  isCellbase: false,
  proposalOrigin: { blockNumber: 930, distance: 2 },
};
const outgoing = {
  proposalIndex: 0,
  proposalId: '0xproposal',
  committedTxHash: '0xtx2',
  committedBlockNumber: 934,
  uncle: null,
};

describe('BlockPosition', () => {
  it('links exact proposal/commitment endpoints and the supplied uncle parent and inclusion', () => {
    const { container } = render(
      <BlockPosition
        block={block}
        transactions={[incoming]}
        proposals={[outgoing]}
        page={1}
        hasMore={false}
      />
    );
    expect(
      container.querySelector('[data-proposal-from="930"][data-proposal-to="932"]')
    ).toBeInTheDocument();
    expect(
      container.querySelector('[data-proposal-from="932"][data-proposal-to="934"]')
    ).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Parent of uncle #931' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${parent}`
    );
    // An uncle is read through the block that included it, at its position there (BA-10).
    expect(screen.getByRole('link', { name: 'Included uncle #931' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/932/uncles/0'
    );
    expect(screen.getByRole('link', { name: 'Block #934' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/934'
    );
  });
  it('counts each committed transaction once across duplicate uncle proposals and labels pagination scope', () => {
    render(
      <BlockPosition
        block={block}
        transactions={[incoming]}
        proposals={[outgoing, { ...outgoing, uncle: { number: 931, hash: uncle } }]}
        page={2}
        hasMore={true}
      />
    );
    expect(screen.getAllByText('1 proposed → committed +2')).toHaveLength(2);
    expect(
      screen.getByText(/Incoming relationships cover transaction page 2 only/)
    ).toBeInTheDocument();
  });
  it('does not fabricate a next canonical block at the tip or an origin for cellbase', () => {
    render(
      <BlockPosition
        block={{ ...block, confirmations: 1, uncles: [], unclesCount: 0 }}
        transactions={[{ hash: '0xbase', isCellbase: true, proposalOrigin: null }]}
        proposals={[]}
        page={1}
        hasMore={false}
      />
    );
    expect(screen.queryByRole('link', { name: 'Block #933' })).not.toBeInTheDocument();
    expect(screen.queryByText(/proposed → committed/)).not.toBeInTheDocument();
  });
  it('provides a vertical mobile reading of the same proposal and uncle evidence', () => {
    render(
      <BlockPosition
        block={block}
        transactions={[incoming]}
        proposals={[outgoing]}
        page={1}
        hasMore={false}
      />
    );
    const mobile = within(screen.getByLabelText('Chain position relationships'));
    expect(mobile.getByRole('link', { name: 'Current block #932' })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${block.hash}`
    );
    // Visual finish V11: the current block's box is the diagram's current tile, a named ink
    // (BLOCK_POSITION_COLORS.currentTile), not a bracket-hex class
    const current = mobile.getByRole('link', { name: 'Current block #932' }).parentElement!;
    expect(current).toHaveStyle({ backgroundColor: 'var(--c-tile-current)' });
    expect(current.className).not.toMatch(/\[#/);
    expect(mobile.getAllByText(fullText('1 proposed → committed · +2 blocks'))).toHaveLength(2);
    expect(mobile.getByRole('link', { name: `Parent ${parent}` })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${parent}`
    );
    expect(
      // U-16 (re-pinned): the uncle line is one separated list, its dot hidden from the name
      mobile.getByRole('link', { name: `Uncle #931 ${uncle}` })
    ).toHaveAttribute('href', '/mainnet/blocks/932/uncles/0');
    expect(screen.getByRole('region', { name: 'Chain position diagram' })).toHaveClass(
      'hidden',
      'md:block'
    );
  });
  it('rejects contradictory proposal distance rather than drawing a repaired relationship', () => {
    expect(() =>
      render(
        <BlockPosition
          block={block}
          transactions={[{ ...incoming, proposalOrigin: { blockNumber: 930, distance: 3 } }]}
          page={1}
          hasMore={false}
        />
      )
    ).toThrow('proposal distance disagrees for 0xtx1');
  });
  it('draws its block tiles, uncle boxes and the uncle’s parent square (U-02, R1)', () => {
    // An uncle whose parent is off screen gets its parent box; one whose parent is the tile beside
    // it forks off that tile in a dashed box of its own.
    const offScreen = render(
      <BlockPosition
        block={block}
        transactions={[incoming]}
        proposals={[outgoing]}
        page={1}
        hasMore={false}
      />
    );
    expect(rects(offScreen.container).length).toBeGreaterThan(5);
    expect(roundedRects(offScreen.container)).toEqual([]);
    offScreen.unmount();
    const beside = render(
      <BlockPosition
        block={block}
        transactions={[incoming]}
        proposals={[outgoing]}
        neighbours={[
          {
            number: 930,
            hash: parent,
            timestamp: '2026-10-26T08:55:20Z',
            transactionSizes: [355],
            proposalsCount: 0,
            unclesCount: 0,
          },
        ]}
        page={1}
        hasMore={false}
      />
    );
    expect(beside.container.querySelector('[data-uncle-fork-from="930"]')).not.toBeNull();
    expect(roundedRects(beside.container)).toEqual([]);
  });
  it('requires proposal evidence for committed non-cellbase transactions', () => {
    expect(() =>
      render(
        <BlockPosition
          block={block}
          transactions={[{ hash: '0xmissing', isCellbase: false, proposalOrigin: null }]}
          page={1}
          hasMore={false}
        />
      )
    ).toThrow('Block 932: missing proposal origin for 0xmissing');
  });
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): every word of the position
// diagram stands whole inside its tile or box and clear of every other word, measured by the type
// modules (0.6 em a glyph; a tight line over its baseline, its descent under it) at the 12 px
// floor. The diagram is drawn at its own width from md up (below md the list above it reads the
// same evidence), so one width is every width it is drawn at. The real case: an 8-digit height,
// two uncles (one forked from the tile beside it, one whose parent is off the strip), proposals
// from 2 and from 10 blocks back, one committed 2 blocks on, and 1,500 transactions in one arc.
describe('BlockPosition: every word inside its tile (readability F3)', () => {
  const at = 21_016_902;
  const hex = (n: number) => `0x${n.toString(16).padStart(64, 'e')}`;
  const neighbour = (number: number) => ({
    number,
    hash: hex(number),
    timestamp: '2026-11-18T08:55:37Z',
    transactionSizes: [635, 226, 1_200, 400],
    proposalsCount: 3,
    unclesCount: 0,
  });
  const many = Array.from({ length: 1_500 }, (_, i) => ({
    hash: `0x${i.toString(16).padStart(64, '0')}`,
    isCellbase: false,
    proposalOrigin: { blockNumber: at - 2, distance: 2 },
  }));
  const real = {
    ...blockEvidence,
    number: at,
    hash: hex(at),
    parentHash: hex(at - 1),
    timestamp: '2026-11-18T08:55:37Z',
    parentTimestamp: '2026-11-18T08:55:26Z',
    confirmations: 30,
    proposalsCount: 1,
    unclesCount: 2,
    uncles: [
      {
        number: at - 1,
        hash: `0x${'b'.repeat(64)}`,
        parentHash: hex(at - 2),
        timestamp: '2026-11-18T08:55:30Z',
        proposalIds: [],
      },
      {
        number: at - 1,
        hash: `0x${'f'.repeat(64)}`,
        parentHash: `0x${'1'.repeat(64)}`,
        timestamp: '2026-11-18T08:55:31Z',
        proposalIds: [],
      },
    ],
  };
  const svgOf = () => {
    const { container } = render(
      <BlockPosition
        block={real}
        transactions={[
          ...many,
          {
            hash: '0xfar',
            isCellbase: false,
            proposalOrigin: { blockNumber: at - 10, distance: 10 },
          },
        ]}
        proposals={[{ ...outgoing, committedBlockNumber: at + 2 }]}
        neighbours={[at - 2, at - 1, at, at + 1, at + 2].map(neighbour)}
        page={1}
        hasMore={false}
      />
    );
    return container.querySelector('[aria-label="Chain position diagram"] svg')!;
  };

  it('writes a block tile’s number, time and contents inside the tile, apart', () => {
    const svg = svgOf();
    const tiles = [...svg.querySelectorAll('g[transform]')];
    expect(tiles.length).toBeGreaterThanOrEqual(6);
    for (const tile of tiles) {
      const box = rectBox(tile.querySelector('rect')!);
      // a height no list row reached (a proposal 10 blocks back) has its number only
      const lines = allTextLines(tile);
      expect(lines.length).toBeGreaterThanOrEqual(1);
      const contents = tile.querySelector('foreignObject');
      const glyph = contents && { ...rectBox(contents), text: 'the contents glyph', size: 0 };
      const all = glyph ? [...lines, glyph] : lines;
      for (const line of all) expectInside(line, box, `tile ${tile.getAttribute('transform')}`);
      expectApart(all, `tile ${tile.getAttribute('transform')}`);
    }
    // the 8-digit height at the head size: "#21,016,902" is 11 × 7.8 = 85.8 px
    expect(allTextLines(svg).some((l) => l.text === '#21,016,902')).toBe(true);
  });

  it('writes an uncle’s and its parent’s words inside their boxes', () => {
    const svg = svgOf();
    const uncles = [...svg.querySelectorAll('[data-uncle-hash]')];
    expect(uncles).toHaveLength(2);
    for (const uncle of uncles)
      for (const link of [...uncle.querySelectorAll('a')]) {
        const box = rectBox(link.querySelector('rect')!);
        const lines = allTextLines(link);
        expect(lines.length).toBeGreaterThanOrEqual(2);
        for (const line of lines) expectInside(line, box, link.getAttribute('aria-label')!);
        expectApart(lines, link.getAttribute('aria-label')!);
      }
  });

  it('keeps every word inside the svg and clear of every other word', () => {
    const svg = svgOf();
    const lines = allTextLines(svg);
    expect(lines.some((l) => l.text === '1500 proposed → committed +2')).toBe(true);
    for (const line of lines) expectInside(line, viewBox(svg), 'the diagram', 'flush');
    expectApart(lines, 'the diagram');
  });
});
