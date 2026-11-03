import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';

describe('DetailLayout identity', () => {
  it('keeps prose words intact while allowing an unbroken identifier to wrap', () => {
    const identity = `Reachability, advertisements and local peer sessions are distinct evidence. 0x${'a'.repeat(64)}`;
    render(
      <DetailLayout title="Network" kind="topic" identity={identity}>
        <p>Network evidence</p>
      </DetailLayout>
    );

    const description = screen.getByText(identity);
    expect(description).toHaveTextContent(identity);
    expect(description).toHaveClass('min-w-0', '[overflow-wrap:anywhere]');
    expect(description).not.toHaveClass('break-all');
  });
});

describe('DetailLayout anatomy (hierarchy plan R1, R6)', () => {
  it('sets the h1 by page kind: index for topic and list pages, entity for entity pages', () => {
    const { rerender } = render(<DetailLayout title="Blocks" kind="index" children="rows" />);
    expect(screen.getByRole('heading', { level: 1 })).toHaveClass('text-index');
    rerender(<DetailLayout title="Nervos DAO" kind="topic" children="board" />);
    expect(screen.getByRole('heading', { level: 1 })).toHaveClass('text-index');
    rerender(<DetailLayout title="Transaction" children="tx" />);
    expect(screen.getByRole('heading', { level: 1 })).toHaveClass('text-entity');
  });

  // Midnight, continued (move 2, BlocksAfter): an index page's title row — h1 and its actions —
  // closes on a lit hairline before the list; an entity or topic page's header has none (its
  // tier-1 h2s carry the marks).
  it('closes an index page title row on a tube; entity and topic headers have none', () => {
    const { container, rerender } = render(
      <DetailLayout title="Blocks" kind="index" actions={<span>legend</span>} children="rows" />
    );
    const header = container.querySelector('header')!;
    const tube = header.querySelector('.hairline-tube') as HTMLElement;
    expect(tube).toHaveAttribute('aria-hidden', 'true');
    expect(tube).toBe(header.lastElementChild);
    expect(screen.getByRole('heading', { level: 1 }).compareDocumentPosition(tube)).toBe(
      Node.DOCUMENT_POSITION_FOLLOWING
    );
    expect(header).toHaveClass('mb-5');
    rerender(<DetailLayout title="Transaction" children="tx" />);
    expect(container.querySelector('header .hairline-tube')).toBeNull();
    expect(container.querySelector('header')).toHaveClass('mb-8');
    rerender(<DetailLayout title="Nervos DAO" kind="topic" children="board" />);
    expect(container.querySelector('header .hairline-tube')).toBeNull();
  });

  it('heads tier-1 sections at section size and tier-2 sections quietly at body, ink', () => {
    render(
      <DetailLayout
        title="Cell"
        tier2={<DetailSection title="Script groups">evidence</DetailSection>}
      >
        <DetailSection title="Capacity">figure</DetailSection>
      </DetailLayout>
    );
    expect(screen.getByRole('heading', { name: 'Capacity' })).toHaveClass(
      'text-section',
      'text-text-bright'
    );
    expect(screen.getByRole('heading', { name: 'Script groups' })).toHaveClass(
      'text-body',
      'text-[#a0a8be]'
    );
  });

  it('opens tier 2 with one boundary: 96 px, a hairline, then the evidence eyebrow above the content', () => {
    const { container } = render(
      <DetailTier2 className="grid lg:grid-cols-2">
        <DetailSection title="The DAO script">script</DetailSection>
      </DetailTier2>
    );
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveClass('mt-24', 'border-t', 'border-base-border', 'pt-7');
    // Midnight, continued (move 2): the boundary's 1 px border-t is a tube — painted by
    // `.hairline-tube-border` (jade at the left, fading right); the border keeps its width.
    expect(tier2).toHaveClass('hairline-tube-border');
    const eyebrow = tier2.firstElementChild as HTMLElement;
    expect(eyebrow).toHaveTextContent(/^Evidence · for developers$/);
    // material pass v2 (move 3): case, tracking and weight are the shared `.eyebrow` rule
    expect(eyebrow).toHaveClass('text-meta', 'text-text-dim', 'eyebrow');
    const heading = screen.getByRole('heading', { name: 'The DAO script' });
    expect(
      eyebrow.compareDocumentPosition(heading) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    // The caller's classes lay out the sections under the eyebrow, not the boundary itself.
    const content = heading.closest('section')!.parentElement!;
    expect(content).toHaveClass('grid', 'lg:grid-cols-2');
    expect(tier2).not.toHaveClass('grid');
  });
});
