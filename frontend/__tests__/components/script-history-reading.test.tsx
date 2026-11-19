import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import { factRows, factsOf } from '@/__tests__/utils/facts';
import { ScriptHistoryReading } from '@/components/scripts/script-family-board';
import type { ScriptFamilyDetail, StackedAreaChartResponse } from '@/lib/api';
import { typeLineage, versionNumbers } from '@/lib/script-lineage';

// The rules branch's review, logic NOTE 6: hover never moves layout (ReadoutRow). A script's
// reading has its "code" fact only on a day some version was deployed by; before the first
// deployment of its lineage it has none, and its row keeps its place unseen, so the reading is as
// tall as its fullest form below md (one column) and from md (two).
const family = fixture.family as unknown as ScriptFamilyDetail;
const lineage = typeLineage(family.history!)!;
const numbers = versionNumbers(family);
const first = lineage[0].indexedDay;
const last = lineage[lineage.length - 1].indexedDay;
const chart = {
  data: [
    {
      date: '2023-10-01',
      values: { used: '100000000', unused: '200000000', capacity: '300000000' },
    },
    { date: last, values: { used: '100000000', unused: '300000000', capacity: '400000000' } },
  ],
} as unknown as StackedAreaChartResponse;

function reading(active: number) {
  const view = render(
    <ScriptHistoryReading chart={chart} lineage={lineage} numbers={numbers} active={active} />
  );
  const el = screen.getByTestId('script-history-reading');
  const out = {
    labels: factsOf(el).map(([label]) => label),
    rows: factRows(el),
    reserved: Array.from(el.querySelectorAll('dl > [aria-hidden="true"]')),
  };
  view.unmount();
  return out;
}

describe('ScriptHistoryReading', () => {
  it('stands as tall with and without its code fact, below md and from md', () => {
    expect('2023-10-01' < first).toBe(true);
    const before = reading(0);
    const after = reading(1);
    expect(after.labels).toContain('code');
    expect(before.labels).not.toContain('code');
    expect(before.rows).toEqual(after.rows);
    expect(before.reserved).toHaveLength(1);
    expect(after.reserved).toHaveLength(0);
  });
});
