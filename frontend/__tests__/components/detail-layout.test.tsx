import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { expectOnePageSeal, expectOnePageSealOf, SEAL_IDS } from '@/__tests__/utils/seals';

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

// Visual finish V7: an entity page's content fades in when its data arrives and the layout mounts.
// Rubbing × Song print, phase 3: an object page presses its hash seal beside its title and
// identity; the facts and the content run under it, never beside the stamp.
describe('DetailLayout seal', () => {
  const HASH = `0x${'a1'.repeat(32)}`;

  it('presses the page seal beside the title and identity, not the facts or the content', () => {
    const { container } = render(
      <DetailLayout
        title="#20,914,858"
        identity={<span>Block {HASH}</span>}
        seal={{ hash: HASH, scale: 'record' }}
        facts={[{ label: 'Epoch', value: '15,271' }]}
      >
        <p>content</p>
      </DetailLayout>
    );
    const seal = expectOnePageSeal(container, HASH, 'record');
    const head = seal.closest('[data-sealed-head]')!;
    const header = container.querySelector('header')!;
    expect(head.parentElement).toBe(header);
    expect(head).toContainElement(screen.getByRole('heading', { level: 1 }));
    expect(head).toHaveTextContent(`Block ${HASH}`);
    expect(head).not.toContainElement(screen.getByText('15,271'));
    expect(header).toContainElement(screen.getByText('15,271'));
    expect(header).not.toContainElement(screen.getByText('content'));
  });

  it('draws no seal when it is given none', () => {
    const { container } = render(<DetailLayout title="Blocks" kind="index" children="rows" />);
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
    expect(container.querySelector('[data-sealed-head]')).toBeNull();
    expect(container.querySelector('header')!.firstElementChild).toContainElement(
      screen.getByRole('heading', { level: 1 })
    );
  });
});

// The seals by kind: an object page presses the stone of its id (a block, a transaction, a cell, a
// script) in its state, in the same place the talisman stood.
describe('DetailLayout seal by kind (the seals by kind)', () => {
  it('presses the stone of the id beside the title and identity, not the facts or the content', () => {
    const block = SEAL_IDS.block;
    const { container } = render(
      <DetailLayout
        title="#20,914,858"
        identity={<span>Block {block.hash}</span>}
        seal={{ id: block }}
        lede={<p data-testid="lede">248 transactions.</p>}
        facts={[{ label: 'Epoch', value: '15,271' }]}
      >
        <p>content</p>
      </DetailLayout>
    );
    const seal = expectOnePageSealOf(container, block);
    const head = seal.closest('[data-sealed-head]')!;
    const header = container.querySelector('header')!;
    expect(head.parentElement).toBe(header);
    const column = seal.nextElementSibling as HTMLElement;
    expect(column).toContainElement(screen.getByRole('heading', { level: 1 }));
    expect(column).toHaveTextContent(`Block ${block.hash}`);
    expect(column.lastElementChild).toContainElement(screen.getByTestId('lede'));
    expect(head).not.toContainElement(screen.getByText('15,271'));
    expect(header).toContainElement(screen.getByText('15,271'));
    expect(header).not.toContainElement(screen.getByText('content'));
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
  });

  it('presses it in the state it is given: a consumed cell in the dead ink, a pool transaction unpressed', () => {
    const { container, unmount } = render(
      <DetailLayout title="Cell" seal={{ id: SEAL_IDS.cell, state: 'ink' }}>
        content
      </DetailLayout>
    );
    expectOnePageSealOf(container, SEAL_IDS.cell, 'ink');
    unmount();
    const pool = render(
      <DetailLayout title="Transaction" seal={{ id: SEAL_IDS.tx, state: 'unpressed' }}>
        content
      </DetailLayout>
    );
    expectOnePageSealOf(pool.container, SEAL_IDS.tx, 'unpressed');
  });
});

// Rubbing × Song print, phase 5 (Gate 3 note): the page's opening sentence reads with its name, the
// head's last line after the identity — beside the seal, so the stamp's height holds the words.
describe('DetailLayout lede', () => {
  const HASH = `0x${'b2'.repeat(32)}`;

  it('sets the lede in the sealed head’s column, after the identity, before the facts and content', () => {
    const { container } = render(
      <DetailLayout
        title="#20,914,858"
        identity={<span data-testid="identity">Block {HASH}</span>}
        seal={{ hash: HASH, scale: 'record' }}
        lede={<p data-testid="lede">4 transactions, mined by a pool.</p>}
        facts={[{ label: 'Epoch', value: '15,271' }]}
      >
        <p>content</p>
      </DetailLayout>
    );
    const seal = expectOnePageSeal(container, HASH, 'record');
    const head = seal.closest('[data-sealed-head]')!;
    const lede = screen.getByTestId('lede');
    // The column beside the seal holds the title row, the identity and then the lede, last.
    const column = seal.nextElementSibling as HTMLElement;
    expect(column).toContainElement(screen.getByRole('heading', { level: 1 }));
    expect(column).toContainElement(lede);
    const holder = lede.parentElement!;
    expect(holder).toHaveAttribute('data-detail-lede');
    expect(holder).toHaveClass('mt-4', 'min-w-0');
    expect(column.lastElementChild).toBe(holder);
    expect(holder.previousElementSibling).toContainElement(screen.getByTestId('identity'));
    expect(head).not.toContainElement(screen.getByText('15,271'));
    expect(container.querySelector('[data-tier="1"]')).not.toContainElement(lede);
  });

  it('sets the lede after the identity in a head without a seal', () => {
    const { container } = render(
      <DetailLayout title="Transaction" identity="0xabc" lede={<p>Two cells became one.</p>}>
        content
      </DetailLayout>
    );
    const header = container.querySelector('header')!;
    const lede = screen.getByText('Two cells became one.').parentElement!;
    expect(header).toContainElement(lede);
    expect(lede.previousElementSibling).toHaveTextContent(/^0xabc$/);
  });

  it('adds nothing to the head when it is given none', () => {
    const { container } = render(
      <DetailLayout
        title="#20,914,858"
        identity={`Block ${HASH}`}
        seal={{ hash: HASH, scale: 'record' }}
      >
        content
      </DetailLayout>
    );
    expect(container.querySelector('[data-detail-lede]')).toBeNull();
    const column = container.querySelector('[data-sealed-head]')!.lastElementChild!;
    expect(column.lastElementChild).toHaveTextContent(`Block ${HASH}`);
  });
});

describe('DetailLayout arrival (visual finish V7)', () => {
  it('carries the arrival fade on its root', () => {
    const { container } = render(<DetailLayout title="Transaction" children="tx" />);
    expect(container.firstElementChild).toHaveClass('arrive');
    expect(container.firstElementChild).toContainElement(
      screen.getByRole('heading', { level: 1, name: 'Transaction' })
    );
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
      'text-text'
    );
    // Rubbing × Song print, phase 2: a tier-1 heading is a command line (PromptHeading: the
    // prompt square is its ::before, the title the lowercase command); tier 2 stays unmarked.
    const capacity = screen.getByRole('heading', { name: 'Capacity' });
    expect(capacity).toHaveClass('prompt');
    expect(capacity.firstElementChild).toHaveClass('lowercase');
    expect(screen.getByRole('heading', { name: 'Script groups' })).not.toHaveClass('prompt');
  });

  it('runs a section command on its args in either tier (phase 5): dim, after the title, in the name', () => {
    render(
      <DetailLayout
        title="Script"
        tier2={
          <DetailSection title="Code versions" args="4">
            evidence
          </DetailSection>
        }
      >
        <DetailSection title="Cells" args="128,440 live" actions={<span>newest first</span>}>
          rows
        </DetailSection>
      </DetailLayout>
    );
    const cells = screen.getByRole('heading', { name: 'Cells 128,440 live' });
    expect(cells).toHaveClass('prompt', 'text-section');
    const [command, args] = Array.from(cells.children) as HTMLElement[];
    expect(command).toHaveTextContent(/^Cells$/);
    expect(args).toHaveTextContent(/^128,440 live$/);
    expect(args).toHaveClass('text-text-dim', 'font-normal', 'normal-case');
    // the actions stay the row's right end, outside the heading
    expect(cells.parentElement).toHaveTextContent('newest first');
    expect(cells).not.toHaveTextContent('newest first');
    // a tier-2 section keeps its quiet h2; the args ride in it, dim, with no prompt
    const versions = screen.getByRole('heading', { name: 'Code versions 4' });
    expect(versions).toHaveClass('text-body', 'text-text');
    expect(versions).not.toHaveClass('prompt');
    expect(versions.lastElementChild).toHaveClass('text-text-dim', 'font-normal');
  });

  it('opens tier 2 with one boundary: 96 px, a hairline, then the evidence eyebrow above the content', () => {
    const { container } = render(
      <DetailTier2 className="grid lg:grid-cols-2">
        <DetailSection title="The DAO script">script</DetailSection>
      </DetailTier2>
    );
    const tier2 = container.querySelector('[data-tier="2"]') as HTMLElement;
    expect(tier2).toHaveClass('mt-24', 'border-t', 'border-base-border', 'pt-7');
    // Midnight, continued (move 2): the boundary's 1 px border-t is coloured by
    // `.hairline-tube-border` (a plain line since rubbing × Song print); the border keeps its width.
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
