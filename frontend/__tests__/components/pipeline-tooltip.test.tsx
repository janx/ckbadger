import { describe, expect, it } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import {
  FIXTURE_NOW,
  FIXTURE_TIP,
  buildPipelineSnapshot,
  poolTxHash,
} from '@/__tests__/msw/fixtures/pipeline-snapshot';
import type { PipelineSnapshot } from '@/lib/api';
import {
  PipelineTooltip,
  txTip,
  windowTip,
  type TipContent,
} from '@/components/pipeline/pipeline-tooltip';

const T = FIXTURE_TIP;
const snapshot = buildPipelineSnapshot();

function renderTip(content: TipContent) {
  render(<PipelineTooltip content={content} />);
  return screen.getByTestId('pipeline-tooltip');
}

const classesOf = (el: Element) => el.className.split(/\s+/);

describe('PipelineTooltip', () => {
  // jsdom does not lay out, so the contract is in the classes: values never wrap, and the tooltip
  // takes the width its rows need instead of squeezing the value column.
  it('keeps the proposal window values on one line', () => {
    const tip = renderTip(windowTip(T + 1, snapshot));
    const range = within(tip).getByText(
      `…${String(T - 9).slice(-4)} – …${String(T - 1).slice(-4)}`
    );
    expect(range.tagName).toBe('DD');
    expect(range).toHaveClass('whitespace-nowrap');
    for (const dd of tip.querySelectorAll('dd')) expect(dd).toHaveClass('whitespace-nowrap');
  });

  it('keeps a long value on one line next to its label, the widest row setting the width', () => {
    const pool = snapshot.pool as NonNullable<PipelineSnapshot['pool']>;
    const early = pool.proposed.find((t) => t.hash === poolTxHash('e0'));
    if (!early) throw new Error('fixture: e0 is not proposed');
    const tip = renderTip(
      txTip({ kind: 'proposed', tx: early, tooEarly: true }, snapshot, FIXTURE_NOW)
    );
    expect(within(tip).getByText('not yet: proposed in the last block')).toHaveClass(
      'whitespace-nowrap'
    );
    const classes = classesOf(tip);
    // Sized by its content, never narrower than the 268 px it had, never wider than the viewport.
    expect(classes).toContain('w-max');
    expect(classes).toContain('min-w-[268px]');
    expect(classes).toContain('max-w-[calc(100vw-16px)]');
    expect(classes.some((c) => /^w-\[/.test(c))).toBe(false);
  });

  it('wraps the footnote to the width the rows set instead of widening the tooltip', () => {
    const tip = renderTip(windowTip(T + 1, snapshot));
    const foot = within(tip).getByText(/A tx can be committed 2 to 10 blocks/);
    expect(foot).toHaveClass('w-0');
    expect(foot).toHaveClass('min-w-full');
    expect(foot).not.toHaveClass('whitespace-nowrap');
  });
});
