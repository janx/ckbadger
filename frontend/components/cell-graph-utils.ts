import type { GraphNode } from '@/lib/api';

export interface FocusCellTarget {
  txHash: string;
  outputIndex: number;
}

export function isFocusedCellNode(
  node: Pick<GraphNode, 'nodeType' | 'data'>,
  focusCell?: FocusCellTarget
): boolean {
  if (!focusCell || node.nodeType !== 'cell') {
    return false;
  }

  return node.data?.txHash === focusCell.txHash && node.data?.outputIndex === focusCell.outputIndex;
}
