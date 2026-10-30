import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { BlockPosition } from '@/components/blocks/block-position';
import { blockEvidence } from '@/__tests__/msw/fixtures/block-evidence';

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
    expect(mobile.getAllByText('1 proposed → committed · +2 blocks')).toHaveLength(2);
    expect(mobile.getByRole('link', { name: `Parent ${parent}` })).toHaveAttribute(
      'href',
      `/mainnet/blocks/${parent}`
    );
    expect(mobile.getByRole('link', { name: `Uncle #931 · ${uncle}` })).toHaveAttribute(
      'href',
      '/mainnet/blocks/932/uncles/0'
    );
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
