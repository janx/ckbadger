import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { rects, roundedRects } from '@/__tests__/utils/square-corners';
import {
  allTextLines,
  expectApart,
  expectInside,
  rectBox,
  textLines,
  viewBox,
} from '@/__tests__/utils/svg-text-boxes';
import { VmLaneTimeline } from '@/components/hardforks/vm-lane-timeline';
import producer from '@/__tests__/fixtures/network-api-producer/hardforks-deployments.json';
import type { HardforkHashTypeLaneF2 } from '@/lib/api';
import {
  PALETTE,
  PALETTE_KEYS,
  THEMES,
  apcaLc,
  contrastRatio,
  inkOnFill,
  overGround,
  type PaletteKey,
  type ThemeName,
} from '@/lib/palette';
import { resetThemeForTests } from '@/lib/theme';

// F2 charts group (CH-40, CH-41): the Hardforks board's VM lanes.

const editions = [
  { shortName: 'Mirana', activationEpoch: 5414, activationDate: '2022-05-10' },
  { shortName: 'Meepo', activationEpoch: 12293, activationDate: '2025-07-01' },
] as const;
const lanes: HardforkHashTypeLaneF2[] = [
  { hashType: 'type', knownVersions: 59, liveReferences: '1830780' },
  { hashType: 'data1', knownVersions: 19, liveReferences: '98551' },
  { hashType: 'data2', knownVersions: 0, liveReferences: '0' },
  { hashType: 'data', knownVersions: 1, liveReferences: '472' },
];
const genesisMs = Date.parse('2019-11-15T21:49:50.812Z');
const tipMs = Date.parse('2026-10-30T00:00:00.000Z');
// R2b-b: each version carries the hash types it is registered under.
const unregistered = producer.scriptDeployments.map((d) => ({ ...d, hashTypes: [] }));
const base = producer.scriptDeployments[0];
const dep = (
  name: string,
  hash: string,
  iso: string,
  hashTypes: ('type' | 'data1' | 'data')[]
) => ({
  ...base,
  versionName: name,
  familyName: name,
  versionHash: hash,
  timestampMs: Date.parse(iso),
  hashTypes,
});
const laned = [
  dep('Default Lock', `0x${'a'.repeat(64)}`, '2019-11-16T00:00:00Z', ['type']),
  dep('xUDT', `0x${'b'.repeat(64)}`, '2024-01-08T00:00:00Z', ['data1']),
  dep('Omni Lock', `0x${'c'.repeat(64)}`, '2023-03-01T00:00:00Z', ['type', 'data1']),
  dep('Old Lock', `0x${'d'.repeat(64)}`, '2020-02-01T00:00:00Z', ['data']),
  dep('Unregistered', `0x${'e'.repeat(64)}`, '2021-02-01T00:00:00Z', []),
];
const texts = () =>
  [...screen.getByRole('img').querySelectorAll('text')].map((node) => node.textContent);

describe('VmLaneTimeline', () => {
  afterEach(() => vi.restoreAllMocks());

  it('keeps every lane, edition and live count readable at phone width', () => {
    stubChartWidth(358);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={unregistered}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    expect(svg).toHaveAttribute('width', '358');
    // Lane names over full-width bars, epoch-only edition labels, numbers in each lane's header.
    // Re-pinned (readability plan P4: a lane is named in plain words, its hash type second): the
    // words are the lane's first line, its hash type and count its second (was "type · the newest
    // VM" beside the count).
    for (const label of [
      'follows the newest VM',
      'hash_type type',
      'pinned to VM1',
      'hash_type data1',
      'pinned to VM2',
      'hash_type data2',
      'pinned to VM0',
      'hash_type data',
      'epoch 5,414',
      'epoch 12,293',
      '1,830,780 · 59\u00a0versions',
    ])
      expect(texts()).toContain(label);
    expect(texts()).not.toContain('epoch 5,414 · 2022-05-10');
    // Re-pinned with PB-02 (UI review 2026-11-14): on a phone the empty lane says so on its own
    // header line, after its count, instead of on its dotted lane line.
    expect(texts()).toContain('0 · no known script yet');
    expect(texts()).not.toContain('no known script yet');
    // Year labels never touch (visual pass at 390: "2020 2021 2022…" ran together).
    const years = [...svg.querySelectorAll('[data-year]')].map((n) => Number(n.getAttribute('x')));
    expect(years.length).toBeGreaterThan(2);
    for (let i = 1; i < years.length; i++)
      expect(years[i] - years[i - 1]).toBeGreaterThanOrEqual(30);
    // The two editions' sub-labels never touch (390 shot: "epoch 5,414" ran into "epoch 12,293").
    const sub = (text: string) => {
      const n = [...svg.querySelectorAll('text')].find((t) => t.textContent === text)!;
      const x = Number(n.getAttribute('x'));
      const w = text.length * 6.5;
      const x0 = n.getAttribute('text-anchor') === 'end' ? x - w : x;
      return { x0, x1: x0 + w, y: Number(n.getAttribute('y')) };
    };
    const [m, e] = [sub('epoch 5,414'), sub('epoch 12,293')];
    expect(m.y !== e.y || m.x1 + 4 <= e.x0 || e.x1 + 4 <= m.x0).toBe(true);
    expect(texts()).not.toContain('live references · known versions');
    // The last edition's label turns left when it would run off the right edge.
    const meepo = [...svg.querySelectorAll('text')].find((n) => n.textContent === 'epoch 12,293')!;
    expect(
      Number(meepo.getAttribute('x')) + 12 * 6.5 <= 358 ||
        meepo.getAttribute('text-anchor') === 'end'
    ).toBe(true);
    // Versions with no registered reference sit on the strip, sharing the plot's insets.
    const strip = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    expect(strip).toHaveStyle({ left: '8px', right: '8px' });
    expect(within(strip).getAllByRole('button')).toHaveLength(2);
  });

  it('stacks an edition block that would crowd the one before it under it, name over epoch', () => {
    // Responsive review A#13: at 390 Meepo's epoch wrapped under Mirana's and read as Mirana's.
    stubChartWidth(358);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={unregistered}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const node = (text: string) =>
      [...screen.getByRole('img').querySelectorAll('text')].find((t) => t.textContent === text)!;
    const at = (text: string) => ({
      x: Number(node(text).getAttribute('x')),
      y: Number(node(text).getAttribute('y')),
      anchor: node(text).getAttribute('text-anchor'),
    });
    const meepo = at('Meepo');
    const meepoEpoch = at('epoch 12,293');
    const miranaEpoch = at('epoch 5,414');
    // Meepo's epoch sits directly under Meepo's name, on the same edge of the same rule.
    expect(meepoEpoch.x).toBe(meepo.x);
    expect(meepoEpoch.anchor).toBe(meepo.anchor);
    expect(meepoEpoch.y).toBeGreaterThan(meepo.y);
    // A clear gap separates Mirana's block from Meepo's: name baselines 18 px below the epoch.
    expect(meepo.y - miranaEpoch.y).toBeGreaterThanOrEqual(18);
    // The lanes start under the last label row (re-pinned, P4: the first lane's first line is its
    // plain words, "follows the newest VM"; it was "type · the newest VM").
    expect(Number(node('follows the newest VM').getAttribute('y'))).toBeGreaterThan(
      meepoEpoch.y + 12
    );
  });

  it('puts each registered version on the lane of every hash type it uses (CH-40, R2b-b)', () => {
    stubChartWidth(1200);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={laned}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    const onLane = (lane: string) =>
      [...svg.querySelectorAll(`[data-lane-dot="${lane}"]`)].map(
        (n) => n.getAttribute('aria-label')!.split(' · ')[0]
      );
    expect(onLane('type')).toEqual(['Default Lock', 'Omni Lock']);
    expect(onLane('data1')).toEqual(['Omni Lock', 'xUDT']);
    expect(onLane('data')).toEqual(['Old Lock']);
    expect(onLane('data2')).toEqual([]);
    // Only the unregistered version stays on the strip below.
    const strip = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    expect(within(strip).getAllByRole('button')).toHaveLength(1);
    expect(screen.getByText(/no registered reference · 1/)).toBeInTheDocument();
    // A lane dot and a strip dot share one evidence panel.
    fireEvent.focus(svg.querySelector('[data-lane-dot="data1"]')!);
    const evidence = screen.getByRole('complementary', {
      name: 'Selected script deployment evidence',
    });
    expect(evidence).toHaveTextContent('Omni Lock');
    // data2 has no known script: its note sits between the two editions, not across Mirana.
    const note = [...svg.querySelectorAll('text')].find(
      (n) => n.textContent === 'not valid before Meepo'
    )!;
    const mirana = [...svg.querySelectorAll('line')].find(
      (n) => n.getAttribute('data-edition') === 'Mirana'
    )!;
    expect(Number(note.getAttribute('x')) - 70).toBeGreaterThan(Number(mirana.getAttribute('x1')));
  });

  it('a tap pins a dot’s script version in the line under the timeline; a second tap clears it (R6)', () => {
    stubChartWidth(1200);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={laned}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    // R9 (re-pinned): the dot's accessible name at the row size, as the reading under it
    const xudt = screen.getByRole('button', {
      name: /^xUDT · xUDT · version 0xbbbbbbbb…bbbbbb, data1/,
    });
    const readout = screen.getByRole('status', { name: 'Script version readout' });
    const evidence = screen.getByRole('complementary', {
      name: 'Selected script deployment evidence',
    });
    expect(svg.compareDocumentPosition(readout) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // R14: the row's own "tap or hover" is the hint; the idle line does not repeat it.
    expect(readout).toHaveTextContent(
      /^a dot reads its script version, its first code cell and its block \(keyboard: focus it\)tap or hover$/
    );
    // a hover still names the version and keeps it once the pointer moves on, as before
    fireEvent.mouseEnter(xudt);
    fireEvent.mouseLeave(xudt);
    expect(readout).toHaveTextContent(
      /^xUDT · xUDT · version 0xbbbbbbbb…bbbbbb · deployed 2024-01-08/
    );
    // a tap pins it; hovering another dot reads that one, leaving it shows the pin again
    fireEvent.click(xudt);
    expect(readout).toHaveTextContent(/^selected xUDT · xUDT/);
    expect(xudt).toHaveAttribute('aria-pressed', 'true');
    const strip = screen.getByRole('list', { name: 'Known script-version deployment timeline' });
    const unregistered = within(strip).getByRole('button');
    fireEvent.mouseEnter(unregistered);
    expect(readout).toHaveTextContent(/^Unregistered · Unregistered/);
    fireEvent.mouseLeave(unregistered);
    expect(readout).toHaveTextContent(/^selected xUDT/);
    expect(evidence).toHaveTextContent('Earliest indexed code-cell creation');
    // the second tap on it lets go
    fireEvent.click(xudt);
    expect(readout).toHaveTextContent(/^a dot reads its script version/);
    expect(evidence).not.toHaveTextContent('Earliest indexed code-cell creation');
  });

  it('names itself inside the plot, top-left, above every edition label (R13)', () => {
    stubChartWidth(1200);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={laned}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    // The page's display element (no defining number): at least 280 px tall.
    expect(Number(svg.getAttribute('height'))).toBeGreaterThanOrEqual(280);
    const name = svg.querySelector('[data-role="chart-name"]')!;
    expect(name).toHaveTextContent(
      /^which VM runs a script, over time · dots: when each known script version was deployed$/
    );
    expect(name.querySelectorAll('tspan')).toHaveLength(0);
    expect(name.getAttribute('x')).toBe('0');
    // re-pinned (readability R4): the label role is 12 px (was 11)
    expect(name.getAttribute('font-size')).toBe('12');
    expect(name.getAttribute('fill')).toBe('var(--c-text-dim)');
    const nameY = Number(name.getAttribute('y'));
    // Edition names (the title role, 13.5 px since R4, was 12.5) and the edition rules start under
    // the name band.
    const y = (text: string) =>
      Number(
        [...svg.querySelectorAll('text')].find((t) => t.textContent === text)!.getAttribute('y')
      );
    for (const label of ['genesis', 'Mirana', 'Meepo'])
      expect(y(label) - 13.5).toBeGreaterThanOrEqual(nameY + 8);
    for (const rule of Array.from(svg.querySelectorAll('line[data-edition]')))
      expect(Number(rule.getAttribute('y1'))).toBeGreaterThanOrEqual(nameY + 8);
  });

  it('wraps the in-plot name into two lines at phone width, both inside the plot', () => {
    stubChartWidth(358);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={unregistered}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    expect(Number(svg.getAttribute('height'))).toBeGreaterThanOrEqual(280);
    const name = svg.querySelector('[data-role="chart-name"]')!;
    const lines = [...name.querySelectorAll('tspan')];
    expect(lines.map((t) => t.textContent)).toEqual([
      'which VM runs a script, over time',
      'dots: when each known script version was deployed',
    ]);
    // re-pinned (readability R4, the one advance model): 12 px at 0.6 em (was 11 px at an
    // observed 0.64 em): each line fits the 358 px plot.
    for (const line of lines) expect(line.textContent!.length * 12 * 0.6).toBeLessThanOrEqual(358);
    const lastY = Number(name.getAttribute('y')) + Number(lines[1].getAttribute('dy'));
    const genesis = [...svg.querySelectorAll('text')].find((t) => t.textContent === 'genesis')!;
    expect(Number(genesis.getAttribute('y')) - 13.5).toBeGreaterThanOrEqual(lastY + 8);
  });

  it.each([358, 1200])(
    'draws each lane’s VM bars with square ends at %i px (U-02, R1)',
    (width) => {
      stubChartWidth(width);
      render(
        <VmLaneTimeline
          editions={editions}
          lanes={lanes}
          deployments={laned}
          genesisMs={genesisMs}
          tipMs={tipMs}
          tipEpoch={15203}
        />
      );
      const svg = screen.getByRole('img');
      expect(rects(svg).length).toBeGreaterThanOrEqual(4);
      expect(roundedRects(svg)).toEqual([]);
    }
  );

  it('refuses lanes out of VM order and hardforks outside the chain span', () => {
    stubChartWidth(1200);
    vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <VmLaneTimeline
          editions={editions}
          lanes={[lanes[1], lanes[0], lanes[2], lanes[3]]}
          deployments={[]}
          genesisMs={genesisMs}
          tipMs={tipMs}
          tipEpoch={15203}
        />
      )
    ).toThrow(/out of VM order/);
    expect(() =>
      render(
        <VmLaneTimeline
          editions={editions}
          lanes={lanes}
          deployments={[]}
          genesisMs={genesisMs}
          tipMs={Date.parse('2024-01-01T00:00:00Z')}
          tipEpoch={9000}
        />
      )
    ).toThrow(/between genesis and the tip/);
  });
});

// Readability page lane P4 (PAGE-LANE-NOTES, F1 follow-up): "VM2" on its bar, jade at 0.55, read
// 3.81 : 1 by night in the bright ink (and 4.13 : 1 in the well's ink: no ink reads on that step),
// written in `text-bright` by hand. A word on a VM bar takes the ink its fill calls for (inkOnFill:
// the well or the bright ink, whichever reads stronger on the fill as the eye sees it over the
// page) and every step of the VM ramp reads at 4.5 : 1 or more in both themes, by night at Lc 60
// or more too (the night grade of a word ink that is not the reading ink): the step moves, the
// palette does not.
function inTheme(theme: ThemeName) {
  localStorage.setItem('ckbadger.theme', theme);
  resetThemeForTests();
  document.documentElement.setAttribute('data-theme', theme);
}
const paletteKey = (ref: string | null): PaletteKey => {
  const key = /^var\(--c-([a-z0-9-]+)\)$/.exec(ref ?? '')?.[1];
  if (!key || !(PALETTE_KEYS as string[]).includes(key))
    throw new Error(`${ref} is not a palette colour`);
  return key as PaletteKey;
};

describe('VmLaneTimeline: a word on a VM bar reads on its fill (P4, F1 follow-up)', () => {
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
    vi.restoreAllMocks();
  });

  it.each(THEMES.flatMap((theme) => [1232, 358].map((width) => [theme, width] as const)))(
    'in the %s theme at %i px',
    (theme, width) => {
      inTheme(theme);
      stubChartWidth(width);
      render(
        <VmLaneTimeline
          editions={editions}
          lanes={lanes}
          deployments={laned}
          genesisMs={genesisMs}
          tipMs={tipMs}
          tipEpoch={15203}
        />
      );
      const svg = screen.getByRole('img');
      const words = [...svg.querySelectorAll('[data-lane] g')].filter((g) =>
        /^VM\d$/.test(g.querySelector('text')?.textContent ?? '')
      );
      expect(words.length).toBe(6);
      const short: string[] = [];
      for (const g of words) {
        const rect = g.querySelector('rect')!;
        const text = g.querySelector('text')!;
        const fill = overGround(
          theme,
          paletteKey(rect.getAttribute('fill')),
          Number(rect.getAttribute('fill-opacity'))
        );
        const ink = paletteKey(text.getAttribute('fill'));
        const ratio = contrastRatio(PALETTE[theme][ink], fill);
        const lc = Math.abs(apcaLc(PALETTE[theme][ink], fill));
        const what = `${text.textContent} at ${rect.getAttribute('fill-opacity')} in ${ink}`;
        if (ratio < 4.5) short.push(`${what}: ${ratio.toFixed(2)} : 1`);
        if (theme === 'dark' && lc < 60) short.push(`${what}: Lc ${lc.toFixed(1)}`);
        // the ink its fill calls for, not one picked by hand
        const called = inkOnFill(
          theme,
          paletteKey(rect.getAttribute('fill')),
          Number(rect.getAttribute('fill-opacity'))
        ).ink;
        if (ink !== called) short.push(`${what}: its fill calls for ${called}`);
      }
      expect(short).toEqual([]);
    }
  );
});

// Readability plan 2026-11-18, lane F3 (the figures' text budgets): at the 12 px floor every word
// of the lanes stands whole inside the svg and clear of every other word — the plot's name, each
// edition's name over its epoch, each lane's head over its note (or its header line on a phone),
// the VM inside its bar, the column head, the years and "now" — measured by the type modules
// (0.6 em a glyph; a tight line over its baseline, its descent under it). The real case is the
// mainnet board's: two editions, 1,830,780 live references on `type`, an empty lane, at the
// hardforks column at 1280 (1232), at 640 (608), and on phones (358, 326).
describe('VmLaneTimeline: every word in its place (readability F3)', () => {
  afterEach(() => vi.restoreAllMocks());

  it.each([1232, 608, 358, 326])('at %i px', (width) => {
    stubChartWidth(width);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={laned}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    const svg = screen.getByRole('img');
    const lines = allTextLines(svg);
    for (const line of lines) expectInside(line, viewBox(svg), 'the lanes', 'flush');
    expectApart(lines, `the lanes at ${width}`);
    // a VM's name inside its bar: three on `type`, one on each other lane
    const bars = [...svg.querySelectorAll('[data-lane] g')].filter((g) =>
      /^VM\d$/.test(g.querySelector('text')?.textContent ?? '')
    );
    expect(bars.length).toBe(6);
    for (const bar of bars)
      for (const line of textLines(bar.querySelector('text')!))
        expectInside(line, rectBox(bar.querySelector('rect')!), `${line.text} in its bar`);
  });
});

// Readability plan 2026-11-18, P4 ("the hardforks rows are named in plain words with hash_type
// second"; the review: the rows were named "hash_type type / data1 / data2 / data", what they mean
// in 11 px dim under them). A lane says first what it is, in the protocol's terms (RFC 0032 /
// 0051: `type` runs on the newest VM the chain runs, `data` pins VM0, `data1` VM1, `data2` VM2), in
// the lane head's ink and size; its hash type is the quiet label under it. On a phone the head is
// a line of its own, and the hash type stands with the lane's count on the line under it.
describe('VmLaneTimeline: each lane named in plain words, its hash type second (P4)', () => {
  afterEach(() => vi.restoreAllMocks());
  const WORDS = {
    type: 'follows the newest VM',
    data1: 'pinned to VM1',
    data2: 'pinned to VM2',
    data: 'pinned to VM0',
  } as const;
  const COUNTS = {
    type: '1,830,780 · 59 versions',
    data1: '98,551 · 19 versions',
    data2: '0 · no known script yet',
    data: '472 · 1 version',
  } as const;
  const draw = (width: number) => {
    stubChartWidth(width);
    render(
      <VmLaneTimeline
        editions={editions}
        lanes={lanes}
        deployments={laned}
        genesisMs={genesisMs}
        tipMs={tipMs}
        tipEpoch={15203}
      />
    );
    return screen.getByRole('img');
  };
  /** A word the lane writes itself (a child of its group), not one of its bars or dots. */
  const own = (lane: Element, text: string) =>
    [...lane.children].find((t) => t.tagName.toLowerCase() === 'text' && t.textContent === text);

  it('on the desktop: the words as the lane head, its hash type the quiet note under them', () => {
    const svg = draw(1232);
    for (const [hashType, words] of Object.entries(WORDS)) {
      const lane = svg.querySelector(`[data-lane="${hashType}"]`)!;
      const head = own(lane, words);
      const note = own(lane, `hash_type ${hashType}`);
      expect(head, `${hashType}: ${words}`).toBeDefined();
      expect(note, `${hashType}: hash_type ${hashType}`).toBeDefined();
      expect(head).toHaveAttribute('font-size', '13');
      expect(head).toHaveAttribute('fill', 'var(--c-text-bright)');
      expect(note).toHaveAttribute('font-size', '12');
      expect(note).toHaveAttribute('fill', 'var(--c-text-dim)');
      expect(Number(head!.getAttribute('x'))).toBe(0);
      expect(Number(note!.getAttribute('x'))).toBe(0);
      expect(Number(note!.getAttribute('y'))).toBeGreaterThan(Number(head!.getAttribute('y')));
    }
  });

  it.each([358, 326])(
    'on a phone (%i px): the words a line of their own, the hash type with the count under them',
    (width) => {
      const svg = draw(width);
      for (const [hashType, words] of Object.entries(WORDS)) {
        const lane = svg.querySelector(`[data-lane="${hashType}"]`)!;
        const head = own(lane, words);
        const note = own(lane, `hash_type ${hashType}`);
        const count = own(lane, COUNTS[hashType as keyof typeof COUNTS]);
        expect(head, `${hashType}: ${words}`).toBeDefined();
        expect(note, `${hashType}: hash_type ${hashType}`).toBeDefined();
        expect(count, `${hashType}: its count`).toBeDefined();
        expect(head).toHaveAttribute('fill', 'var(--c-text-bright)');
        expect(note).toHaveAttribute('fill', 'var(--c-text-dim)');
        // the head stands alone on its line; the hash type and the count share the next one
        expect(Number(note!.getAttribute('y'))).toBeGreaterThan(Number(head!.getAttribute('y')));
        expect(count!.getAttribute('y')).toBe(note!.getAttribute('y'));
      }
    }
  );
});
