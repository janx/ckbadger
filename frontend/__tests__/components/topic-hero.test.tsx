import { describe, expect, it } from 'vitest';
import { render, screen, within } from '../utils/test-utils';
import { TopicHero } from '@/components/ui/topic-hero';

describe('TopicHero (Fiber / Forks board hero)', () => {
  it('sets the index title and a body lede beside a grid of hero figures with meta labels, in order (R1)', () => {
    render(
      <TopicHero
        title="Forks"
        lede="Two miners sometimes find a block at the same height."
        figures={[
          { label: 'Uncle rate · 30 days', value: '2.57%', sub: '7,178 uncles in 279,561 blocks' },
          { label: 'This node switched branch', value: '0 times', tone: 'jade' },
        ]}
      />
    );
    const title = screen.getByRole('heading', { level: 1, name: 'Forks' });
    expect(title).toHaveClass('text-index');
    expect(screen.getByText('Two miners sometimes find a block at the same height.')).toHaveClass(
      'text-body'
    );
    const terms = screen.getAllByRole('term');
    expect(terms.map((t) => t.textContent)).toEqual([
      'Uncle rate · 30 days',
      'This node switched branch',
    ]);
    for (const term of terms) expect(term).toHaveClass('text-meta', 'text-text-dim');
    expect(screen.getByText('2.57%')).toHaveClass('text-hero', 'text-text-bright');
    expect(screen.getByText('0 times')).toHaveClass('text-jade');
    expect(screen.getByText('7,178 uncles in 279,561 blocks')).toHaveClass('text-meta');
  });

  it('renders no figure grid while the figures are still loading', () => {
    render(<TopicHero title="Fiber channels" lede="Lede." figures={undefined} />);
    expect(screen.queryByRole('term')).toBeNull();
  });
});

// Hierarchy v3 (plan §1b, R11, R13): the Fiber and Forks heroes read like the DAO page — title,
// one short lede, the page's one display figure with its companion, the picture under it, the
// picture's reading, then the other figures as one quiet row.
describe('TopicHero figure layout (hierarchy v3, R11, R13)', () => {
  const quiet = [
    { label: 'Channels ever', value: '345', sub: 'since 2025-02-28 · 1 in USDI' },
    { label: 'Parties', value: '25', sub: 'in 35 pairs' },
    { label: 'Typical lifetime', value: '3 m 21 s' },
  ];
  const hero = (figure?: Parameters<typeof TopicHero>[0]['figure']) =>
    render(
      <TopicHero
        layout="figure"
        title="Fiber channels"
        lede="A Fiber channel touches the chain only when it opens and when it closes."
        figure={figure}
        picture={<svg data-testid="picture" />}
        reading={<p data-testid="reading">reading</p>}
        figures={quiet}
      />
    );

  it('stacks title, lede, figure and picture in one hero section; reading and the quiet row follow it (R13)', () => {
    hero({
      label: 'open now',
      value: '38',
      unit: 'channels',
      companion: 'locking 43,889 CKB',
      tone: 'jade',
    });
    const section = screen.getByRole('region', { name: 'Fiber channels' });
    expect(section.tagName).toBe('SECTION');
    const title = within(section).getByRole('heading', { level: 1, name: 'Fiber channels' });
    expect(title).toHaveClass('text-index');
    // the title row, as the DAO page's: the h1 is a flex item, its box the size of its words
    expect(title.parentElement).toHaveClass('flex', 'items-baseline', 'justify-between');
    const lede = within(section).getByText(/^A Fiber channel touches the chain/);
    expect(lede).toHaveClass('text-body', 'text-text-dim', 'max-w-[560px]');
    expect(lede.textContent!.length).toBeLessThanOrEqual(90);
    // the one display figure, its unit a step quieter, its companion on the same baseline row
    const value = within(section).getByText('38');
    expect(value).toHaveClass('text-index', 'sm:text-display', 'text-jade', 'tabular-nums');
    const unit = within(section).getByText('channels');
    expect(unit).toHaveClass('text-hero', 'text-text-dim');
    expect(value.contains(unit) || value.parentElement!.contains(unit)).toBe(true);
    const companion = within(section).getByText('locking 43,889 CKB');
    expect(companion).toHaveClass('text-body');
    const row = companion.parentElement!;
    expect(row).toHaveClass('flex', 'items-baseline', 'justify-between');
    expect(row.contains(value)).toBe(true);
    expect(within(section).getByText('open now')).toHaveClass('text-meta', 'uppercase', 'w-fit');
    // the picture directly under the figure, inside the hero section
    const picture = within(section).getByTestId('picture');
    expect(row.compareDocumentPosition(picture) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    // the reading and the quiet row follow the section, in that order
    const reading = screen.getByTestId('reading');
    expect(section.contains(reading)).toBe(false);
    expect(
      section.compareDocumentPosition(reading) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    const terms = screen.getAllByRole('term');
    expect(terms.map((t) => t.textContent)).toEqual([
      'Channels ever',
      'Parties',
      'Typical lifetime',
    ]);
    const dl = terms[0].closest('dl')!;
    expect(section.contains(dl)).toBe(false);
    expect(reading.compareDocumentPosition(dl) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('sets the other figures as one quiet row: meta label, section value, inline dim note (R13)', () => {
    hero({ label: 'open now', value: '38', tone: 'jade' });
    for (const term of screen.getAllByRole('term')) expect(term).toHaveClass('text-meta');
    const values = screen.getAllByRole('definition');
    // one dd per figure: the note sits inline in it, never a second line
    expect(values).toHaveLength(3);
    for (const dd of values) expect(dd).toHaveClass('text-section', 'text-text-bright', 'w-fit');
    expect(values[0]).toHaveTextContent(/^345 · since 2025-02-28 · 1 in USDI$/);
    expect(within(values[0]).getByText(/since 2025-02-28/)).toHaveClass(
      'text-small',
      'text-text-dim'
    );
    expect(values[2]).toHaveTextContent(/^3 m 21 s$/);
    // nothing in the row is set at the hero step any more
    expect(values[0].closest('dl')!.querySelector('.text-hero')).toBeNull();
  });

  it('keeps the figure the only coloured text: bright where the page has no class colour (R11)', () => {
    hero({ label: 'uncle rate', value: '2.58', unit: '%', companion: '7,138 uncles' });
    expect(screen.getByText('2.58')).toHaveClass('text-text-bright');
    const section = screen.getByRole('region', { name: 'Fiber channels' });
    expect(section.querySelector('.text-jade')).toBeNull();
  });

  it('draws no figure row while the figure loads, but keeps the title, the lede and the picture', () => {
    hero(undefined);
    const section = screen.getByRole('region', { name: 'Fiber channels' });
    expect(within(section).getByRole('heading', { level: 1 })).toBeInTheDocument();
    expect(within(section).getByTestId('picture')).toBeInTheDocument();
    expect(section.querySelector('[data-role="figure"]')).toBeNull();
  });
});
