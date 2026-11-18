import { afterEach, describe, expect, it, vi } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { atViewportWidth } from '@/__tests__/utils/viewport';
import { containerContentPx } from '@/__tests__/utils/tailwind-box';
import { lineRuns } from '@/__tests__/utils/line-runs';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import {
  ActivityRow,
  ActivityTitle,
  activityTitleText,
  type ActivityRowData,
} from '@/components/activity/activity-row';
import { ActivityRuns, runSubject } from '@/components/activity/activity-runs';
import { CLOCK_BEHIND } from '@/lib/format-age';

/**
 * Lane B4, the coordinator's third item: an activity row's right part (its evidence: the block
 * number or the pool's status, then the age) held its whole run in one `whitespace-nowrap`, so
 * "this device's clock is behind" stood on one line with the block number. `Utc8Age` is the one
 * owner of an age's white-space (a figure is one unit, that sentence breaks between its words) and
 * no caller wraps a run around it; the unbreakable units own themselves.
 *
 * The property, read from the rendered row with the page's clock two minutes behind the chain:
 * every run of the right part that a line cannot break inside fits the row's line, the page's
 * content width (358 at 390, 328 at 360, 288 at 320: the row has no stone), and those runs are the
 * units themselves. Fix round (review-rules-tests, the coordinator's item 5): the right part is a
 * flex item, and it was `shrink-0`, so it stood at its whole width and broke nowhere: the reader
 * now reads an item that cannot shrink as one run, and the row is read at 320 too, where the
 * committed row's 309.6 px and the pool row's 326.4 px pass the 288 px line. At the meta step (12 px, 7.2 a glyph) the committed row's evidence joined the block
 * number to the sentence's first word, "#21,047,791 · this", 18 glyphs, 129.6 px (Utc8Age lets
 * the sentence break inside itself, the row's run did not); the pool row's was one run, the
 * "Confirming" badge (13 px, 10 glyphs, 78, its 8 px sides and 1 px borders: 96), " · " (21.6) and
 * the sentence's 29 glyphs (208.8): 326.4 px, 1.6 px inside the 328 px line at 360, the whole
 * status and its sentence held as one. Now the widest run of either is the badge, 96 px, or the
 * block number, 11 glyphs, 79.2 px; the folded row's widest, "device's", 57.6 px.
 */

/** Two minutes ahead of this device's clock: further than consensus allows (15 s). */
const ahead = () => String(Date.now() + 2 * 60_000);

function row(over: Partial<ActivityRowData> = {}): ActivityRowData {
  return {
    txHash: `0x${'c1'.repeat(32)}`,
    blockNumber: 21047791,
    timestamp: ahead(),
    isCellbase: false,
    protocolActions: [],
    typeCalls: [],
    lockCalls: [],
    participants: [
      {
        address: seededAddress('q3q7ue'),
        lockHash: `0x${'01'.repeat(32)}`,
        lockHashPrefix: null,
        lockKind: 'Default Lock',
        roles: [],
        tags: 0,
        ckbDelta: '-30000',
        usedDelta: '0',
        itemDeltas: [],
      },
    ],
    ...over,
  };
}

const pool = (): ActivityRowData =>
  row({
    blockNumber: null,
    timestamp: null,
    poolStatus: 'committed_awaiting_index',
    timeAddedToPool: new Date(Date.now() + 2 * 60_000).toISOString(),
  });

const SENTENCE = CLOCK_BEHIND.split(' ');

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

/** The runs of a row's right part, and those wider than its line. */
function runsOf(container: HTMLElement, viewport: number) {
  const evidence = container.querySelector('[data-role="evidence"]') as HTMLElement;
  const runs = lineRuns(evidence, viewport);
  const line = containerContentPx(viewport);
  return {
    texts: runs.map((r) => r.text),
    past: runs.filter((r) => r.px > line + 1e-9).map((r) => `${r.text} ${r.px.toFixed(1)}`),
  };
}

describe.each([390, 360, 320])(
  'an activity row’s right part with the clock behind, at %i px (B4)',
  (viewport) => {
    it('the committed row: the block number one unit, the sentence words', () => {
      atViewportWidth(viewport);
      const { container } = render(<ActivityRow activity={row()} age />);
      const { texts, past } = runsOf(container, viewport);
      // the property first (nothing past the line), then the units it is made of
      expect(past).toEqual([]);
      expect(texts).toEqual(['#21,047,791', '·', ...SENTENCE]);
    });

    it('the pool row: the status one unit, the sentence words', () => {
      atViewportWidth(viewport);
      const { container } = render(<ActivityRow activity={pool()} age />);
      const { texts, past } = runsOf(container, viewport);
      // the property first (nothing past the line), then the units it is made of
      expect(past).toEqual([]);
      expect(texts).toEqual(['Confirming', '·', ...SENTENCE]);
    });

    it('the folded row: "newest" and the sentence, words', () => {
      atViewportWidth(viewport);
      const rows = [row(), row({ txHash: `0x${'c2'.repeat(32)}`, blockNumber: 21047790 })];
      const { container } = render(
        <ActivityRuns
          age
          rows={rows.map((a) => ({
            key: a.txHash,
            subject: runSubject(a, activityTitleText(a)),
            activity: a,
            title: <ActivityTitle activity={a} />,
            row: <ActivityRow activity={a} age />,
          }))}
        />
      );
      const run = container.querySelector('[data-role="activity-run"]') as HTMLElement;
      const { texts, past } = runsOf(run, viewport);
      // the property first (nothing past the line), then the units it is made of
      expect(past).toEqual([]);
      expect(texts).toEqual(['newest', ...SENTENCE]);
    });
  }
);
