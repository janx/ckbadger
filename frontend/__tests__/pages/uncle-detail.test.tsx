import { Component, type ReactNode } from 'react';
import { describe, expect, it, vi } from 'vitest';
import { act, fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { onlineManager } from '@tanstack/react-query';
import { http, HttpResponse } from 'msw';
import { server } from '@/__tests__/msw/server';
import { blockUncle } from '@/__tests__/msw/fixtures/block-uncle';
import UncleDetailPage from '@/app/blocks/[id]/uncles/[index]/client-page';
import { loadUnderStillClock, T0, tick } from '@/__tests__/utils/still-clock';
import { expectAnswerUnclipped } from '@/__tests__/utils/copy-answer';
import { factsOf } from '@/__tests__/utils/facts';
import { expectBefore, expectNamePlate, expectOnePageSeal } from '@/__tests__/utils/seals';
import { inkOf, READING } from '@/__tests__/utils/ink-role';

const params = vi.hoisted(() => ({ current: { id: '8775638', index: '0' } }));
vi.mock('@/src/navigation', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/src/navigation')>()),
  useParams: () => params.current,
  usePathname: () => `/blocks/${params.current.id}/uncles/${params.current.index}`,
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div>Header</div> }));

class Boundary extends Component<{ children: ReactNode }, { error: Error | null }> {
  state = { error: null as Error | null };
  static getDerivedStateFromError(error: Error) {
    return { error };
  }
  render() {
    return this.state.error ? <p role="alert">{this.state.error.message}</p> : this.props.children;
  }
}

describe('Uncle detail page', () => {
  // Lane D2b (U-03, PA-06; the owner's ruling of 2026-11-19): the uncle page takes the block's name
  // plate — its stone beside the kind line "Uncle", the h1 and the id line with its copy, then the
  // status strip and the lede under them — the stone the block stone WITHOUT rods (the chain keeps
  // no transactions for an uncle) in the dead ink; one plain sentence says why it has no rods.
  it('stands its name plate beside its block stone without rods, in the dead ink (lane D2b)', async () => {
    params.current = { id: '8775638', index: '0' };
    const { container } = render(<UncleDetailPage />);
    const h1 = await screen.findByRole('heading', { level: 1, name: '#8,775,637' });
    const seal = expectOnePageSeal(
      container,
      { kind: 'block', hash: blockUncle.hash, txs: 'not kept' },
      'ink'
    );
    expect(seal.querySelector('svg')).toHaveAttribute(
      'aria-label',
      `Block seal ${blockUncle.hash.slice(0, 10)}, the chain keeps no transaction count for an uncle`
    );
    const { stone, kindLine, title, idLine, below } = expectNamePlate(container, 'Uncle');
    expect(stone).toBe(seal);
    expect(title).toBe(h1);
    expect(h1).toHaveClass('text-entity', 'max-md:text-index');
    expect(kindLine).toHaveTextContent(/^Uncle$/);
    expect(idLine).toContainElement(screen.getByRole('button', { name: 'copy uncle hash' }));
    // under the stone and the plate: the strip, then the lede on the measure, which says once why
    // the stone has no rods
    expect(below!.children).toHaveLength(2);
    expect(below!.firstElementChild).toBe(screen.getByRole('group', { name: 'Uncle status' }));
    const lede = below!.lastElementChild as HTMLElement;
    expect(lede).toHaveAttribute('data-detail-lede');
    expect(lede).toHaveClass('max-w-measure');
    expect(within(lede).getByText(/^Its seal has no rods/)).toHaveTextContent(
      'Its seal has no rods: a block’s seal counts its transactions in rods, and the chain keeps none for an uncle.'
    );
  });

  // Lane D2 (PA-13, PA-06; gate D0′'s strip, with the uncle's own four facts): the block that
  // included it, its time, the canonical block at its height and its epoch stand in the status
  // strip under the id line, and its sentence follows the strip as every entity page's lede does
  // (the facts stood before the head's sentence, which was the page's first content).
  it('states included by, time, canonical and epoch in one strip under the id line, then its sentence (lane D2)', async () => {
    params.current = { id: '8775638', index: '0' };
    const { container } = render(<UncleDetailPage />);
    const strip = await screen.findByRole('group', { name: 'Uncle status' });
    const facts = factsOf(strip);
    // Re-pinned on purpose (lane P1 follow-up; review chain-a uncle: the label wrote a number and
    // its value was a bare hash, only the prose said what it was): the label names the block, the
    // uncle's sibling, and its note says it is the canonical block on the same parent. A label no
    // wider than "included by": below sm the strip is a grid of labels and values, its label column
    // as wide as its widest label, and at 320 the time ("2026-11-14 13:05:16 UTC+8", 195 px at 13
    // px) holds the 288 px column beside 11 characters of label (79.2 px at 12 px) and the 12 px
    // gap; "canonical at its height" (165.6 px) put it 69 px past the column (Chrome, 320)
    expect(facts.map(([label]) => label)).toEqual(['included by', 'time', 'sibling', 'epoch']);
    expect(Math.max(...facts.map(([label]) => label.length))).toBe('included by'.length);
    expect(facts[0][1]).toBe('#8,775,638 · uncle 0');
    expect(facts[1][1]).toMatch(/^2026-02-22 07:59:40 UTC\+8 · \S+ ago · 92 ms after its sibling$/);
    expect(facts[2][1]).toBe(`0x${'bb'.repeat(4)}…${'bb'.repeat(3)} · canonical, same parent`);
    expect(facts[3][1]).toBe('5,414 · 1 / 1,800');
    // the uncle's state in the dead ink
    expect(strip.querySelector<HTMLElement>('[data-strip-dot]')!.style.backgroundColor).toBe(
      'var(--c-text-dim)'
    );
    // the id line, the strip, then the sentence: in the head, in that order
    const header = container.querySelector('header') as HTMLElement;
    const sentence = screen.getByText(/^Its sibling on the canonical chain/);
    expect(header).toContainElement(strip);
    expect(header).toContainElement(sentence);
    expectBefore(container.querySelector('[data-head-id]') as HTMLElement, strip);
    expectBefore(strip, sentence);
  });

  it('reads the uncle through its including block, with header and DAO evidence', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    // PB-06 / PA-32: an entity h1 takes the phone step (30 → 26 px below md). Re-pinned on purpose
    // (lane D2b): the h1 is the height alone, "#8,775,637", as the block page's; the kind line
    // above it says "Uncle" (it was "Uncle #8,775,637", with no kind line), here and below
    expect(await screen.findByRole('heading', { name: '#8,775,637' })).toHaveClass(
      'text-entity',
      'max-md:text-index'
    );
    // Re-pinned on purpose (lane P1 follow-up; review chain-a uncle: a 158-character paragraph):
    // two sentences, the sibling and the parent, then the including block and what an uncle is
    const sentence = screen.getByText(/^Its sibling on the canonical chain/);
    expect(sentence).toHaveTextContent(
      /^Its sibling on the canonical chain, 0x\S+, was mined on the same parent, #8,775,636, and this uncle came 92 ms after it\. #8,775,638 included it: its transactions never ran, but the proposals it carried count toward the proposal window\.$/
    );
    expect(within(sentence).getByRole('link', { name: '#8,775,638' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/8775638'
    );
    expect(screen.getByText('uncle 0', { exact: false })).toBeInTheDocument();
    expect(screen.getAllByText('2026-02-22 07:59:40 UTC+8').length).toBeGreaterThan(0);
    expect(screen.getAllByText('5,414 · 1 / 1,800')).toHaveLength(2);
    expect(screen.getByText('0xaabbccddeeff00112233')).toBeInTheDocument();
    expect(screen.getByText('1,210,829,853,049,231,829')).toBeInTheDocument();
    const decoded = screen.getByLabelText('Decoded DAO header');
    expect(decoded).toHaveTextContent('65,862,172,919.14818600');
    expect(decoded).toHaveTextContent('1.2065094518945692');
    expect(decoded).toHaveTextContent('S · compensation still owed + the burnt share');
    expect(screen.getByText(blockUncle.dao.raw)).toBeInTheDocument();
    // readability R3 / R4 (lane B7): the uncle's sentence and the DAO field note stand on the
    // measure (they ran 158 and 148 characters a line at 1280); the note explains, so it reads in
    // the reading ink at the body step (it was the quiet ink at the raw data's small step)
    // re-pinned on purpose (lane D2): the sentence is the head's lede now, set on the measure by
    // the lede's one holder (the paragraph carried the measure itself in the page's content)
    expect(sentence.closest('[data-detail-lede]')).toHaveClass('max-w-measure');
    const note = within(decoded).getByText(/^S counts the burnt share/);
    expect(note).toHaveClass('text-text', 'text-body', 'max-w-measure');
    expect(note).not.toHaveClass('text-text-dim');
  });

  // The review's item E: the plate's id is the uncle's copy control; a click writes the WHOLE hash
  // Re-pinned (lane c2-finish, item 4: copy has one form): the id is text; the copy button after
  // it copies the whole hash (the id itself was the click target, its tick over it).
  it('copies the whole uncle hash when the plate’s id is clicked', async () => {
    params.current = { id: '8775638', index: '0' };
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    const { container } = render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: '#8,775,637' });
    const id = container.querySelector('[data-head-id]') as HTMLElement;
    fireEvent.click(id);
    expect(writeText).not.toHaveBeenCalled();
    // lane c2-finish, rulings round 1: the copy's answer stands out of the line's flow under
    // its box, and nothing above the copy clips it
    expectAnswerUnclipped(screen.getByRole('button', { name: 'copy uncle hash' }));
    fireEvent.click(screen.getByRole('button', { name: 'copy uncle hash' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(blockUncle.hash));
    expect(blockUncle.hash).toBe(`0x${'ee'.repeat(32)}`);
  });

  it('writes the header epoch like the facts and never breaks DAO prose mid-word (BA-41/BA-42)', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: '#8,775,637' });
    // facts strip + raw header row: one epoch format on the page
    expect(screen.getAllByText('5,414 · 1 / 1,800')).toHaveLength(2);
    const decoded = screen.getByLabelText('Decoded DAO header');
    expect(decoded.closest('.break-all')).toBeNull();
    expect(screen.getByText(blockUncle.dao.raw)).toHaveClass('break-all');
  });

  it('shows the checked endpoint for an index the block does not have', async () => {
    params.current = { id: '8775638', index: '3' };
    render(<UncleDetailPage />);
    // the checked endpoint stands in the facts (the eyebrow shows the route, which here reads the same)
    expect(
      (await screen.findAllByText('/blocks/8775638/uncles/3')).some((el) =>
        el.closest('.error-facts')
      )
    ).toBe(true);
    expect(screen.getByText('no uncle at this block and index.')).toBeInTheDocument();
  });

  it('refuses decoded DAO fields that disagree with the raw header bytes', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, dao: { ...blockUncle.dao, s: '0' } })
      )
    );
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(
      <Boundary>
        <UncleDetailPage />
      </Boundary>
    );
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'DAO S disagrees with the raw header bytes'
    );
    spy.mockRestore();
  });

  // F2 blocks (BA-43): the sibling relation, read from R10's sameParentAs / msAfterSibling.
  it('names the canonical block at its height as its sibling on the same parent, and the delay', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: '#8,775,637' });
    // re-pinned (lane P1 follow-up): the sentence names the sibling first
    const sentence = screen.getByText(/^Its sibling on the canonical chain/);
    expect(sentence).toHaveTextContent(
      /Its sibling on the canonical chain, 0x.+, was mined on the same parent, #8,775,636, and this uncle came 92 ms after it\./
    );
    expect(screen.getByText('92 ms after its sibling')).toBeInTheDocument();
  });

  it('says the canonical block at its height has another parent when they are not siblings', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, sameParentAs: null, msAfterSibling: null })
      )
    );
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: '#8,775,637' });
    // re-pinned (lane P1 follow-up): the canonical block at its height, which is no sibling
    const sentence = screen.getByText(/^The canonical block at its height/);
    expect(sentence).toHaveTextContent(
      /^The canonical block at its height, 0x\S+, has another parent; this uncle was mined on 0x\S+\. #8,775,638 included it:/
    );
    expect(sentence).not.toHaveTextContent('sibling');
  });

  // lane P1, the ink and size roles: an empty line is a sentence, the reading ink (it was quiet)
  it('says an uncle that proposed nothing in the reading ink', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, proposalIds: [] })
      )
    );
    render(<UncleDetailPage />);
    expect(inkOf(await screen.findByText('This uncle proposed nothing.'))).toBe(READING);
  });

  it('refuses sibling facts that contradict each other', async () => {
    params.current = { id: '8775638', index: '0' };
    server.use(
      http.get('/api/:network/v1/blocks/:id/uncles/:index', () =>
        HttpResponse.json({ ...blockUncle, msAfterSibling: null })
      )
    );
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    render(
      <Boundary>
        <UncleDetailPage />
      </Boundary>
    );
    expect(await screen.findByRole('alert')).toHaveTextContent('sibling facts disagree');
    spy.mockRestore();
  });

  it('shows the packed epoch with its reading in the header (BA-41, R2b-a rawEpoch)', async () => {
    params.current = { id: '8775638', index: '0' };
    render(<UncleDetailPage />);
    await screen.findByRole('heading', { name: '#8,775,637' });
    expect(screen.getByText('0x7080001001526')).toBeInTheDocument();
    expect(screen.getByText('0x7080001001526').parentElement).toHaveTextContent(
      '0x7080001001526 = 5,414 · 1 / 1,800'
    );
  });

  // Lane E4: the Time fact's age reads the page's one clock, so it moves with no data change; it
  // was written once at render and stood still. The clock stops with the page.
  it('moves the Time fact’s age with no data change, and leaves no timer behind (E4)', async () => {
    params.current = { id: '8775638', index: '0' };
    vi.useFakeTimers({ now: T0 });
    try {
      server.use(
        http.get('*/blocks/:id/uncles/:index', () =>
          HttpResponse.json({ ...blockUncle, timestamp: new Date(T0 - 20_000).toISOString() })
        )
      );
      const { unmount } = render(<UncleDetailPage />);
      const age = () => screen.queryByText(/^\d+[smh] ago$/)?.textContent;
      await loadUnderStillClock(() => age() !== undefined);
      expect(age()).toBe('20s ago');
      tick(1000);
      expect(age()).toBe('21s ago');
      tick(38_000);
      expect(age()).toBe('59s ago');
      tick(1000);
      expect(age()).toBe('1m ago');
      unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  // Review fix: a read paused between retries (a hidden tab, the browser offline) is pending
  // without fetching, so isLoading is false: the page drew nothing at all while it waited.
  it('says it is loading while its read is paused offline, then shows the uncle once back online', async () => {
    params.current = { id: '8775638', index: '0' };
    onlineManager.setOnline(false);
    try {
      render(<UncleDetailPage />);
      expect(await screen.findByText('Loading uncle…')).toHaveAttribute('role', 'status');
      act(() => onlineManager.setOnline(true));
      expect(await screen.findByRole('heading', { name: '#8,775,637' })).toBeInTheDocument();
      expect(screen.queryByText('Loading uncle…')).toBeNull();
    } finally {
      onlineManager.setOnline(true);
    }
  });
});
