import { describe, expect, it } from 'vitest';
import { screen } from '@testing-library/react';
import { render } from '../utils/test-utils';
import { DetailLayout, DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import {
  expectBefore,
  expectNamePlate,
  expectOnePageSeal,
  SEAL_IDS,
} from '@/__tests__/utils/seals';

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
// The seals by kind: an object page presses the stone of its id (a block, a transaction, a cell, a
// script) in its state; the facts and the content run under the head, never beside the stamp.
// The name plate (C10): with a seal the head is the plate — the kind line (the kind word the seal
// names, then `status`), the h1 and `identity` beside the stone, `lede` under both from the page's
// left edge (`below`).
describe('DetailLayout seal', () => {
  it('makes the head the name plate: the kind word and status, the h1 and identity, the lede below', () => {
    const block = SEAL_IDS.block;
    const { container } = render(
      <DetailLayout
        title="#20,914,858"
        identity={<span data-testid="identity">{block.hash}</span>}
        seal={{ id: block, kind: 'Block' }}
        status={<a href="/hardforks">HARDFORK ACTIVATION · MIRANA</a>}
        lede={<p data-testid="lede">248 transactions.</p>}
        facts={[{ label: 'Epoch', value: '15,271' }]}
      >
        <p>content</p>
      </DetailLayout>
    );
    const seal = expectOnePageSeal(container, block);
    const { head, kindLine, nameBox, title, idLine, below } = expectNamePlate(container, 'Block');
    const header = container.querySelector('header')!;
    expect(head.parentElement).toBe(header);
    expect(header.firstElementChild).toBe(head);
    expect(seal.parentElement).toBe(head);
    // the kind line: the kind word, then the status
    expect(kindLine.children).toHaveLength(2);
    expect(kindLine.lastElementChild).toBe(screen.getByRole('link'));
    expect(kindLine).toHaveTextContent(/^BlockHARDFORK ACTIVATION · MIRANA$/);
    // the name and the identity, nothing else: the h1 (no status beside it) and the id line, in
    // the plate's one quiet voice (C10 round 2), set by the plate around `identity`
    expect(title).toHaveTextContent(/^#20,914,858$/);
    expect(nameBox.children).toHaveLength(2);
    expect(nameBox.lastElementChild).toBe(idLine);
    expect(idLine.children).toHaveLength(1);
    expect(idLine.firstElementChild).toBe(screen.getByTestId('identity'));
    expect(idLine.className).toBe('text-text-dim text-small min-w-0 [overflow-wrap:anywhere]');
    // the plate's gap spaces it, so the identity carries no margin of its own
    expect(idLine).not.toHaveClass('mt-2');
    // the lede is what is below, with no holder of its own
    expect(below!.children).toHaveLength(1);
    expect(below!.firstElementChild).toBe(screen.getByTestId('lede'));
    expect(container.querySelector('[data-detail-lede]')).toBeNull();
    // the facts and the content stay out of the head; the facts follow it in the header
    expect(head).not.toContainElement(screen.getByText('15,271'));
    expect(header).toContainElement(screen.getByText('15,271'));
    expect(header).not.toContainElement(screen.getByText('content'));
    expect(container.querySelector('[data-hash-seal]')).toBeNull();
  });

  // RESPONSIVE_LAYOUT principle 5 (one step smaller at most): under the stone below md the name has
  // the full width, but at 30 px a short out point (19 characters, 330.6 px) and a short address
  // (21, 365.4 px) are wider than the 328 px line at 360; at 26 px, 286.5 and 316.7.
  it('sets a sealed entity’s name one type step smaller below md, and only a sealed one', () => {
    const { rerender } = render(
      <DetailLayout
        title="0xbe1b9761…936cd8:1"
        identity={`${SEAL_IDS.cell.tx}:1`}
        seal={{ id: SEAL_IDS.cell, kind: 'Cell' }}
      >
        content
      </DetailLayout>
    );
    const title = screen.getByRole('heading', { level: 1 });
    expect(title).toHaveClass(
      'text-text-bright',
      'break-words',
      'text-entity',
      'max-md:text-index'
    );
    expect(title.className).not.toMatch(/(^|\s)(sm|md|lg|xl):text-/);
    rerender(<DetailLayout title="0xbe1b9761…936cd8:1">content</DetailLayout>);
    expect(screen.getByRole('heading', { level: 1 }).className).toBe(
      'text-text-bright break-words text-entity'
    );
  });

  it('presses it in the state it is given: a consumed cell in the dead ink, a pool transaction unpressed', () => {
    const { container, unmount } = render(
      <DetailLayout
        title="Cell"
        identity={`${SEAL_IDS.cell.tx}:1`}
        seal={{ id: SEAL_IDS.cell, state: 'ink', kind: 'Cell' }}
      >
        content
      </DetailLayout>
    );
    expectOnePageSeal(container, SEAL_IDS.cell, 'ink');
    unmount();
    const pool = render(
      <DetailLayout
        title="Transaction"
        identity={SEAL_IDS.tx.hash}
        seal={{ id: SEAL_IDS.tx, state: 'unpressed', kind: 'Transaction' }}
      >
        content
      </DetailLayout>
    );
    expectOnePageSeal(pool.container, SEAL_IDS.tx, 'unpressed');
  });

  it('draws no seal when it is given none', () => {
    const { container } = render(<DetailLayout title="Blocks" kind="index" children="rows" />);
    expect(container.querySelector('[data-seal]')).toBeNull();
    expect(container.querySelector('[data-sealed-head]')).toBeNull();
    expect(container.querySelector('header')!.firstElementChild).toContainElement(
      screen.getByRole('heading', { level: 1 })
    );
  });

  // A sealed head is the plate, which has no actions slot and always its id line; every other page
  // keeps its title row.
  it('refuses actions, or no identity, on a sealed head at compile time', () => {
    const withActions = {
      title: '#1',
      identity: SEAL_IDS.block.hash,
      seal: { id: SEAL_IDS.block, kind: 'Block' },
      actions: <span>legend</span>,
      children: 'content',
    };
    // @ts-expect-error a sealed head (the name plate) has no actions slot
    const sealed = <DetailLayout {...withActions} />;
    // the same props without the actions are a sealed head
    const { actions: _actions, ...withoutActions } = withActions;
    void _actions;
    const plain = <DetailLayout {...withoutActions} />;
    expect(plain.props.seal.kind).toBe('Block');
    // @ts-expect-error a sealed head says its kind word
    const unnamed = <DetailLayout title="#1" seal={{ id: SEAL_IDS.block }} children="content" />;
    const { identity: _identity, ...withoutIdentity } = withoutActions;
    void _identity;
    // @ts-expect-error a sealed head has its id line
    const unidentified = <DetailLayout {...withoutIdentity} />;
    expect(sealed.props.seal.kind).toBe('Block');
    expect(unnamed.props.seal).toEqual({ id: SEAL_IDS.block });
    expect(unidentified.props.identity).toBeUndefined();
  });

  // Every page without a seal (indexes, topics, the entity pages that press none) keeps the head as
  // it was before the name plate: the title row (h1 and status, then actions), the identity, the
  // lede, then the facts. Pinned as the markup itself, taken from the code before C10.
  it('renders an unsealed head exactly as before the name plate', () => {
    const { container, unmount } = render(
      <DetailLayout
        title="Network"
        kind="topic"
        identity="Reachability and sessions."
        status={<span>3 sources</span>}
        actions={<span>legend</span>}
        lede={<p>Two cells became one.</p>}
        facts={[{ label: 'Epoch', value: '15,271' }]}
      >
        content
      </DetailLayout>
    );
    expect(container.querySelector('header')!.outerHTML).toBe(
      '<header class="mb-8"><div class="flex flex-wrap items-baseline justify-between gap-3">' +
        '<div class="flex min-w-0 flex-wrap items-baseline gap-3">' +
        '<h1 class="text-text-bright break-words text-index">Network</h1><span>3 sources</span>' +
        '</div><span>legend</span></div>' +
        '<div class="text-text-dim text-small mt-2 min-w-0 [overflow-wrap:anywhere]">' +
        'Reachability and sessions.</div>' +
        '<div data-detail-lede="" class="mt-4 min-w-0"><p>Two cells became one.</p></div>' +
        '<dl class="mt-6 grid grid-cols-1 gap-x-12 gap-y-3 md:grid-cols-2">' +
        '<div class="grid min-w-0 grid-cols-1 items-baseline gap-x-3 gap-y-1 sm:grid-cols-[minmax(80px,1fr)_minmax(0,2fr)]">' +
        '<dt class="text-text-dim text-small min-w-0 break-words">Epoch</dt>' +
        '<dd class="text-text-bright min-w-0 break-words tabular-nums">15,271</dd></div></dl></header>'
    );
    unmount();
    const entity = render(
      <DetailLayout
        title={<span>Transaction</span>}
        documentTitle={['Transaction']}
        identity={<span>0xabc</span>}
      >
        content
      </DetailLayout>
    );
    expect(entity.container.querySelector('header')!.outerHTML).toBe(
      '<header class="mb-8"><div class="flex flex-wrap items-baseline justify-between gap-3">' +
        '<div class="flex min-w-0 flex-wrap items-baseline gap-3">' +
        '<h1 class="text-text-bright break-words text-entity"><span>Transaction</span></h1>' +
        '</div></div>' +
        '<div class="text-text-dim text-small mt-2 min-w-0 [overflow-wrap:anywhere]">' +
        '<span>0xabc</span></div></header>'
    );
  });
});

// Rubbing × Song print, phase 5 (Gate 3 note): the page's opening sentence reads with its name, the
// head's last line after the identity. With a seal (C10) it is under the stone and the name plate,
// from the page's left edge, where every section under the head starts (it was set beside the
// stone, indented by the stone's width).
describe('DetailLayout lede', () => {
  const HASH = SEAL_IDS.block.hash;

  it('sets the lede under the stone and the plate in a sealed head, after the identity, before the facts', () => {
    const { container } = render(
      <DetailLayout
        title="#20,914,858"
        identity={<span data-testid="identity">{HASH}</span>}
        seal={{ id: SEAL_IDS.block, kind: 'Block' }}
        lede={<p data-testid="lede">4 transactions, mined by a pool.</p>}
        facts={[{ label: 'Epoch', value: '15,271' }]}
      >
        <p>content</p>
      </DetailLayout>
    );
    const { head, plate, below } = expectNamePlate(container, 'Block');
    const lede = screen.getByTestId('lede');
    expect(below).toContainElement(lede);
    expect(head.lastElementChild).toBe(below);
    expect(plate).not.toContainElement(lede);
    expectBefore(screen.getByTestId('identity'), lede);
    expectBefore(lede, screen.getByText('15,271'));
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
        identity={HASH}
        seal={{ id: SEAL_IDS.block, kind: 'Block' }}
      >
        content
      </DetailLayout>
    );
    expect(container.querySelector('[data-detail-lede]')).toBeNull();
    const { head, plate, nameBox, below } = expectNamePlate(container, 'Block');
    expect(below).toBeNull();
    expect(head.lastElementChild).toBe(plate);
    expect(nameBox.lastElementChild).toHaveTextContent(new RegExp(`^${HASH}$`));
    expect(nameBox.lastElementChild).toHaveAttribute('data-sealed-id');
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
