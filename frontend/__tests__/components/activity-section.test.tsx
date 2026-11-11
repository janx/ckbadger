import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ActivitySection } from '@/components/home/activity-section';
import { ACTIVITY_HEAD_CLASS } from '@/components/home/section';

// Rubbing × Song print, phase 5 (H4 / H5): the feed and the figures each open on a command line —
// `■ feed --last 8` and `■ activity --24 h` — in one heading-row class, so where the two columns
// stand side by side the two lines sit at one height. The section keeps its one name, "Activity".
describe('ActivitySection', () => {
  beforeEach(() => stubChartWidth(560));
  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('heads both columns with the same heading row, first in each column', () => {
    render(<ActivitySection />);
    const section = screen.getByRole('region', { name: 'Activity' });
    expect(section).not.toHaveAttribute('aria-labelledby');
    const feed = screen.getByRole('heading', { level: 2, name: 'feed --last 8' });
    const stats = screen.getByRole('heading', { level: 2, name: 'activity --24 h' });
    const rowOf = (h: HTMLElement) => h.parentElement!.parentElement as HTMLElement;
    const [feedRow, statsRow] = [rowOf(feed), rowOf(stats)];
    // one class string, character for character: the same height, the same centring
    expect(feedRow.className).toBe(ACTIVITY_HEAD_CLASS);
    expect(statsRow.className).toBe(feedRow.className);
    expect(screen.getByTestId('home-feed').firstElementChild).toBe(feedRow);
    expect(screen.getByTestId('home-activity-stats').firstElementChild).toBe(statsRow);
    // the columns themselves start together in the section's grid
    expect(Array.from(section.children)).toEqual([
      screen.getByTestId('home-feed'),
      screen.getByTestId('home-activity-stats'),
    ]);
  });
});
