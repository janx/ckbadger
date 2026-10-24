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
            share: 0.814,
            href: '/charts/activity-type-breakdown',
          },
          { key: 'dao', label: 'DAO', share: 0.0011 },
        ]}
      />
    );
    expect(screen.getByRole('link', { name: 'By type, share of 7,115' })).toHaveAttribute(
      'href',
      '/mainnet/charts/activity-type-breakdown'
    );
    const script = screen.getByRole('link', { name: /Script call/ });
    expect(script).toHaveTextContent('81.4%');
    expect(script.className).toContain('grid-cols-[92px_minmax(0,1fr)_52px]');

    const fills = document.querySelectorAll<HTMLElement>('[data-role="share-fill"]');
    expect(fills[0].style.width).toBe('81.4%');
    expect(fills[0]).toHaveClass('bg-jade', 'h-1.5', 'min-w-[2px]');
    expect(fills[0].parentElement).toHaveClass('bg-base-border-subtle', 'h-1.5');

    // A row with nowhere to go is not a link.
    const dao = document.querySelector('[data-rank-row="dao"]') as HTMLElement;
    expect(dao.querySelector('a')).toBeNull();
    expect(dao).toHaveTextContent('DAO0.1%');
  });

  it('gives a linked row a hover state: the row lifts and its label brightens', () => {
    render(
      <RankedBars
        title="By type"
        rows={[
          { key: 'script', label: 'Script call', share: 0.814, href: '/x' },
          { key: 'dao', label: 'DAO', share: 0.0011 },
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
        rows={[{ key: 'a', label: '.bit Time Index State', share: 0.485, href: '/scripts/x' }]}
        wideLabels
      />
    );
    const row = screen.getByRole('link', { name: /.bit Time Index State/ });
    expect(row.className).toContain('grid-cols-[minmax(0,1.3fr)_minmax(0,1fr)_52px]');
    expect(screen.getByText('.bit Time Index State')).toHaveClass('truncate');
  });
});
