import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { RankedBars } from '@/components/home/ranked-bars';

describe('RankedBars', () => {
  it('draws each share as a jade fill on a 6 px track, with its percentage, linking where it can', () => {
    render(
      <RankedBars
        title="By type, share of 7,115"
        titleHref="/charts/activity-type-breakdown"
        rows={[
          {
            key: 'script',
            label: 'Script call',
            count: 814,
            total: 1000,
            href: '/charts/activity-type-breakdown',
          },
          { key: 'dao', label: 'DAO', count: 11, total: 10000 },
        ]}
      />
    );
    expect(screen.getByRole('link', { name: 'By type, share of 7,115' })).toHaveAttribute(
      'href',
      '/mainnet/charts/activity-type-breakdown'
    );
    const script = screen.getByRole('link', { name: /Script call/ });
    // CH-14 (re-pinned): the one percentage of its count in the total, two decimals (was '81.4%')
    expect(script).toHaveTextContent('81.40%');
    expect(script.className).toContain('grid-cols-[92px_minmax(0,1fr)_52px]');

    const fills = document.querySelectorAll<HTMLElement>('[data-role="share-fill"]');
    // the width from the exact parts at two decimals; the style reads it back without its 0
    expect(fills[0].style.width).toBe('81.4%');
    expect(fills[0]).toHaveClass('bg-jade', 'h-1.5', 'min-w-[2px]');
    // the empty track is the bar-track ink, which clears the ruled paper by day (R5, re-pinned on
    // purpose: it was the hair, 1.09:1 on a ruled line)
    expect(fills[0].parentElement).toHaveClass('bg-bar-track', 'h-1.5');

    // A row with nowhere to go is not a link.
    const dao = document.querySelector('[data-rank-row="dao"]') as HTMLElement;
    expect(dao.querySelector('a')).toBeNull();
    expect(dao).toHaveTextContent('DAO0.11%');
  });

  it('gives a linked row a hover state: the row lifts and its label brightens', () => {
    render(
      <RankedBars
        title="By type"
        rows={[
          { key: 'script', label: 'Script call', count: 814, total: 1000, href: '/x' },
          { key: 'dao', label: 'DAO', count: 11, total: 10000 },
        ]}
      />
    );
    const row = screen.getByRole('link', { name: /Script call/ });
    expect(row).toHaveClass(
      'group',
      'hover:bg-base-elevated/60',
      'focus-visible:bg-base-elevated/60'
    );
    expect(screen.getByText('Script call')).toHaveClass('group-hover:text-text-bright');
    // A row with nowhere to go gets no hover state.
    const dao = document.querySelector('[data-rank-row="dao"] > div') as HTMLElement;
    expect(dao.className).not.toContain('hover:');
  });

  it('gives long script names a flexible, truncating column', () => {
    render(
      <RankedBars
        title="By script, share of 11,300 calls"
        rows={[
          { key: 'a', label: '.bit Time Index State', count: 485, total: 1000, href: '/scripts/x' },
        ]}
        wideLabels
      />
    );
    const row = screen.getByRole('link', { name: /.bit Time Index State/ });
    expect(row.className).toContain('grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_52px]');
    expect(screen.getByText('.bit Time Index State')).toHaveClass('truncate');
  });
});
