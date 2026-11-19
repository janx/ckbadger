import { render, screen } from '@/__tests__/utils/test-utils';
import { CellVersions } from '@/components/asset/cell-versions';
import { dotbitV4ProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';
import { ckbAddress } from '@/__tests__/utils/ckb-address';
import { fireEvent, within } from '@testing-library/react';
import type { ItemCellVersion } from '@/lib/api';

it('prints each version capacity once with its unit, never "CKB CKB" (OB-3, ID-C3)', () => {
  const cells = dotbitV4ProtocolProducer.versions.map((version) => version.cell);
  render(<CellVersions history={{ data: cells, total: cells.length }} subject="alice.bit" />);
  const list = screen.getByLabelText('alice.bit cell versions');
  expect(list.textContent).not.toMatch(/CKB\s*CKB/);
  expect(list).toHaveTextContent('CKB');
});

// Rubbing × Song print, phase 5 (D13, D12): the default section is the versions, run on their
// count; a caller that names the section (the boards' "CKB held") gives the heading whole.
it('runs "cell versions" on the count of versions, and a named section on nothing', () => {
  const cells = dotbitV4ProtocolProducer.versions.map((version) => version.cell);
  const history = { data: cells, total: cells.length };
  const { unmount } = render(<CellVersions history={history} subject="alice.bit" />);
  expectPrompt(
    screen.getByRole('heading', { name: `Cell versions ${cells.length}` }),
    'Cell versions',
    String(cells.length)
  );
  // the note stays the row's right end, outside the heading
  expect(screen.getByText('Every canonical cell version · oldest first')).not.toBe(
    screen.getByRole('heading')
  );
  expect(screen.getByRole('heading')).not.toHaveTextContent('oldest first');
  unmount();
  render(
    <CellVersions
      history={history}
      subject="alice.bit"
      title="CKB held"
      actions={<span>1 CKB per byte</span>}
    />
  );
  expectPrompt(screen.getByRole('heading', { name: 'CKB held' }), 'CKB held');
  expect(screen.getByRole('heading').parentElement).toHaveTextContent('1 CKB per byte');
});

// Lane P2's third round (items 2 and 3): a version row names its owner only where the owner
// changes from the version before (the first names it); consecutive versions that differ in
// nothing a reader can see (capacity, bytes, owner) fold under the one fold control, the folded
// row saying how many and what they share; the current version and a version that changes
// anything are never folded.
describe('cell versions: the owner where it changes, alike versions folded', () => {
  const base = dotbitV4ProtocolProducer.versions[0].cell as ItemCellVersion;
  const ownerOf = (args: string) => ({
    lock: { ...base.lock, args },
    ownerAddress: ckbAddress({
      codeHash: base.lock.codeHash,
      hashType: base.lock.hashType as 'type',
      args,
    }),
  });
  const A = ownerOf(`0x${'0a'.repeat(20)}`);
  const B = ownerOf(`0x${'0b'.repeat(20)}`);
  const version = (
    i: number,
    owner: typeof A,
    capacity: string,
    last = false
  ): ItemCellVersion => ({
    ...base,
    ...owner,
    txHash: `0x${String(i).padStart(2, '0').repeat(32)}`,
    outputIndex: 0,
    blockNumber: 100 + i,
    txIndex: 0,
    capacity,
    isLive: last,
    consumedByTx: last
      ? null
      : `0x${String(i + 1)
          .padStart(2, '0')
          .repeat(32)}`,
    consumedAtBlock: last ? null : 101 + i,
  });
  const held = (v: ItemCellVersion) => v.occupiedCapacity;
  const extra = (v: ItemCellVersion, n: number) => String(BigInt(held(v)) + BigInt(n));

  it('names the owner on the first version and where it changes, not again where it stays', () => {
    const cells = [
      version(1, A, extra(base, 1)),
      version(2, A, extra(base, 2)),
      version(3, B, extra(base, 3), true),
    ];
    render(<CellVersions history={{ data: cells, total: 3 }} subject="alice.bit" />);
    const rows = within(screen.getByLabelText('alice.bit cell versions')).getAllByRole('article');
    expect(rows.map((r) => /Owner/.test(r.textContent ?? ''))).toEqual([true, false, true]);
  });

  it('folds alike versions between the first of their run and the current one, and opens them', () => {
    const cells = [
      version(1, A, extra(base, 1)),
      version(2, A, extra(base, 9)),
      version(3, A, extra(base, 9)),
      version(4, A, extra(base, 9)),
      version(5, A, extra(base, 9), true),
    ];
    render(<CellVersions history={{ data: cells, total: 5 }} subject="alice.bit" />);
    const list = screen.getByLabelText('alice.bit cell versions');
    expect(within(list).getAllByRole('article')).toHaveLength(3);
    const fold = list.querySelector<HTMLElement>('[data-versions-fold]')!;
    expect(fold).toHaveTextContent(
      /^2\smore versions alike: .+ CKB held, the same bytes and owner/
    );
    const toggle = within(fold).getByRole('button', { name: /^all 2\sversions ↓$/ });
    toggle.focus();
    fireEvent.click(toggle);
    expect(within(list).getAllByRole('article')).toHaveLength(5);
    expect(toggle).toHaveAccessibleName('fewer ↑');
    expect(document.activeElement).toBe(toggle);
  });

  it('never folds a version that changes anything, nor a run of alike versions too short to fold', () => {
    const cells = [
      version(1, A, extra(base, 1)),
      version(2, A, extra(base, 9)),
      version(3, A, extra(base, 9)),
      version(4, B, extra(base, 9)),
      version(5, B, extra(base, 7), true),
    ];
    render(<CellVersions history={{ data: cells, total: 5 }} subject="alice.bit" />);
    const list = screen.getByLabelText('alice.bit cell versions');
    expect(within(list).getAllByRole('article')).toHaveLength(5);
    expect(list.querySelector('[data-versions-fold]')).toBeNull();
  });
});
