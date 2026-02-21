import { describe, expect, it } from 'vitest';
import type { GraphNode } from '@/lib/api';
import { isFocusedCellNode } from '@/components/cell-graph-utils';

function makeCellNode(txHash: string, outputIndex: number): Pick<GraphNode, 'nodeType' | 'data'> {
  return {
    nodeType: 'cell',
    data: { txHash, outputIndex },
  };
}

describe('isFocusedCellNode', () => {
  it('returns true when node matches focused cell outpoint', () => {
    const node = makeCellNode('0xabc', 1);
    expect(isFocusedCellNode(node, { txHash: '0xabc', outputIndex: 1 })).toBe(true);
  });

  it('returns false when output index does not match', () => {
    const node = makeCellNode('0xabc', 1);
    expect(isFocusedCellNode(node, { txHash: '0xabc', outputIndex: 2 })).toBe(false);
  });

  it('returns false for non-cell nodes', () => {
    const node: Pick<GraphNode, 'nodeType' | 'data'> = {
      nodeType: 'transaction',
      data: { txHash: '0xabc', outputIndex: 1 },
    };
    expect(isFocusedCellNode(node, { txHash: '0xabc', outputIndex: 1 })).toBe(false);
  });
});
