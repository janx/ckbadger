// F2 group `objects` (round 2): the shared hero pieces of the Object and Mnft boards.
import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import {
  COMPOSITION_SENTENCES,
  LiveBadge,
  ObjectTraits,
  cellLifeSentence,
  itemLifeSentence,
  itemLifeAges,
  lockFootnote,
  spelledDuration,
} from '@/components/asset/object-hero';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { ckbAddress, JOYID_CODE_HASH } from '@/__tests__/utils/ckb-address';
import { sporeCellVersionsProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ItemCellVersion } from '@/lib/api';

const v = (over: Partial<ItemCellVersion>): ItemCellVersion => ({
  ...sporeCellVersionsProducer.data[0],
  ...over,
});

it('spells durations and lock phrases for prose', () => {
  expect(spelledDuration(30_000)).toBe('30\u00a0seconds');
  expect(spelledDuration(60_000)).toBe('1\u00a0minute');
  expect(spelledDuration(2 * 86_400_000)).toBe('2\u00a0days');
  expect(spelledDuration(800 * 86_400_000)).toBe('2\u00a0years');
  expect(() => spelledDuration(-1)).toThrow('invalid duration');
});

// Re-pinned on purpose (lane P2's follow-up, item 6): the life sentence says the lock a cell sits
// under through the one decision's sentence form (lockPhrase in components/ui/lock-kind.tsx), off
// the cell's lock and its owner's address, where it said the API's name with its own article rule:
// a lock the registry does not name was "an unregistered lock" and is its code hash, and a lock on
// the default lock's code hash by data, which the registry's name does not cover, was called the
// default lock's name and is its code hash by data.
it('says the lock a cell sits under through the decision’s sentence form', () => {
  const at = (lockName: string | null) => v({ timestamp: '0', lockName, isLive: true });
  expect(itemLifeSentence([at('Nervape Shadow Lock')])).toBe(
    'Minted to a Nervape Shadow Lock, where it sits now.'
  );
  expect(itemLifeSentence([at('secp256k1')])).toBe(
    'Minted to a secp256k1 lock, where it sits now.'
  );
  expect(itemLifeSentence([at('Omnilock')])).toBe('Minted to an Omnilock, where it sits now.');
  expect(itemLifeSentence([at(null)])).toBe(
    'Minted to a lock 0x41414141…414141, where it sits now.'
  );
  const args = `0x${'01'.repeat(20)}`;
  const byData = v({
    timestamp: '0',
    isLive: true,
    lock: { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'data', args },
    lockName: 'Default Lock',
    ownerAddress: ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'data', args }),
  });
  expect(itemLifeSentence([byData])).toBe(
    'Minted to a lock 0x9bd7e06f…a3cce8 by data, where it sits now.'
  );
  // the footnote says the same words without the article
  expect(lockFootnote([at(null), at(null)], 'object')).toBe(
    'Lock: lock 0x41414141…414141 throughout. Type and data never changed.'
  );
  // a cell whose address spells another lock is refused, the cell named
  expect(() =>
    itemLifeSentence([{ ...byData, lock: { ...byData.lock, hashType: 'type' } }])
  ).toThrow(`cell ${byData.txHash}:${byData.outputIndex}`);
});

// Lane P (after lane F's C3): the sentence tells the path; its dates are the page's Minted and Last
// moved metrics (C7) and how long ago they were is its hover (itemLifeAges) — no "ago" in text.
// Lane E4 (re-pinned): the sentence takes no "now", as it writes no time of the reader's.
it('tells an item’s life from its cell versions', () => {
  const minted = v({ timestamp: '0', lockName: 'secp256k1', isLive: false });
  const moved = v({ timestamp: '30000', lockName: 'Nervape Shadow Lock', isLive: true });
  expect(itemLifeSentence([{ ...moved, timestamp: '0' }])).toBe(
    'Minted to a Nervape Shadow Lock, where it sits now.'
  );
  expect(itemLifeSentence([minted, moved])).toBe(
    'Minted to a secp256k1 lock, then moved 30\u00a0seconds later to a Nervape Shadow Lock, where it sits now.'
  );
  expect(
    itemLifeSentence([minted, { ...moved, isLive: false, consumedAtBlock: 12 }], 'Issued', 'X')
  ).toBe('Issued by X to a secp256k1 lock, moved 1 time; its last cell was consumed at block 12.');
  const third = v({ timestamp: String(8 * 86_400_000), lockName: 'JoyID', isLive: true });
  expect(itemLifeSentence([minted, { ...moved, isLive: false }, third])).toBe(
    'Minted to a secp256k1 lock, then moved 2 times; the last move put it under a JoyID lock, where it sits now.'
  );
  expect(itemLifeSentence([minted, moved])).not.toMatch(/ ago/);
  expect(() => itemLifeSentence([])).toThrow('at least one cell version');
});

// Readability R8 (lane p-assets): as an object page's lede the sentence says what it is and
// whether it is live, and names no figure the metrics row states (C7): no count of moves, no span
// of time, no block.
it('tells an item’s life as a lede: its path and its state, no figure', () => {
  const minted = v({ timestamp: '0', lockName: 'secp256k1', isLive: false });
  const moved = v({ timestamp: '30000', lockName: 'Nervape Shadow Lock', isLive: true });
  const third = v({ timestamp: String(8 * 86_400_000), lockName: 'JoyID', isLive: true });
  const lede = (versions: ItemCellVersion[]) =>
    itemLifeSentence(versions, 'Minted', undefined, true);
  expect(lede([{ ...moved, timestamp: '0' }])).toBe(
    'Minted to a Nervape Shadow Lock, where it sits now.'
  );
  expect(lede([minted, moved])).toBe(
    'Minted to a secp256k1 lock, then moved to a Nervape Shadow Lock, where it sits now.'
  );
  expect(lede([minted, { ...moved, isLive: false }, third])).toBe(
    'Minted to a secp256k1 lock, then moved; the last move put it under a JoyID lock, where it sits now.'
  );
  expect(lede([minted, { ...moved, isLive: false, consumedAtBlock: 12_345_678 }])).toBe(
    'Minted to a secp256k1 lock, then moved; its last cell was consumed, so it is gone.'
  );
  for (const versions of [
    [minted, moved],
    [minted, { ...moved, isLive: false }, third],
    [minted, { ...moved, isLive: false, consumedAtBlock: 12_345_678 }],
  ])
    expect(lede(versions)).not.toMatch(/moved \d|later|block|12,345,678/);
});

it('keeps how long ago an item was minted and last moved for the sentence’s hover', () => {
  const now = 10 * 86_400_000;
  const minted = v({ timestamp: '0', lockName: 'secp256k1', isLive: false });
  const third = v({ timestamp: String(8 * 86_400_000), lockName: 'JoyID', isLive: true });
  expect(itemLifeAges([minted], now)).toBe('minted 10\u00a0days ago');
  expect(itemLifeAges([minted, third], now, 'Issued')).toBe(
    'issued 10\u00a0days ago · last moved 2\u00a0days ago'
  );
});

// A4 review 1 (the sweep of computed counts): an age is this device's clock minus a block's header
// time, and a node's clock is not this device's; two header times need not run forward either
// (a block's timestamp only has to pass the median of the 37 before it). Neither may throw.
it('reads an age through the one age rule when this device’s clock is behind', () => {
  const minted = v({ timestamp: '600000', lockName: 'secp256k1', isLive: false });
  const moved = v({ timestamp: '900000', lockName: 'JoyID', isLive: true });
  // a minute behind the mint: no age, the clock-behind sentence
  expect(itemLifeAges([minted], 540_000)).toBe("minted: this device's clock is behind");
  expect(itemLifeAges([minted, moved], 660_000, 'Issued')).toBe(
    "issued 1 minute ago · last moved: this device's clock is behind"
  );
  // within the 15 s consensus allows: age zero
  expect(itemLifeAges([minted], 590_000)).toBe('minted under a second ago');
});

it('tells a move without a duration when the later header time runs backwards', () => {
  const minted = v({ timestamp: '900000', lockName: 'secp256k1', isLive: false });
  const moved = v({ timestamp: '880000', lockName: 'Nervape Shadow Lock', isLive: true });
  expect(itemLifeSentence([minted, moved])).toBe(
    'Minted to a secp256k1 lock, then moved to a Nervape Shadow Lock, where it sits now.'
  );
});

// Re-pinned on purpose (lane P2's fifth round, item 2; review-fix-logic-pages MUST 2): the
// sentence states what the route sent, both capacities and both sizes of a part that differs, and
// over how many moves, never a difference the page computed ("so the cell grew by 13 CKB when it
// moved", which named one move over several); and it says what is true of the state the cell is
// in: a live cell "holds", a consumed one "held … when it was consumed", nothing in the present.
describe('the cell’s life in one sentence, in the state it is in', () => {
  const cell = (capacity: number, lockBytes: number, over: Partial<ItemCellVersion> = {}) =>
    v({
      capacity: `${capacity}00000000`,
      occupiedCapacity: `${capacity}00000000`,
      anatomy: { capacityBytes: 8, lockBytes, typeBytes: 65, dataBytes: capacity - lockBytes - 73 },
      isLive: false,
      ...over,
    });
  it('says the live cell holds its CKB, and what minting held, over its one move', () => {
    const a = v({
      capacity: '30700000000',
      occupiedCapacity: '30700000000',
      anatomy: { capacityBytes: 8, lockBytes: 53, typeBytes: 65, dataBytes: 181 },
      isLive: false,
    });
    const b = v({
      capacity: '32000000000',
      occupiedCapacity: '32000000000',
      anatomy: { capacityBytes: 8, lockBytes: 66, typeBytes: 65, dataBytes: 181 },
      isLive: true,
    });
    expect(cellLifeSentence([a, b], 'object')).toBe(
      'The object’s cell holds 320 CKB. At minting it held 307 CKB, its lock 53\u00a0bytes; after 1\u00a0move its lock is 66\u00a0bytes.'
    );
    expect(
      cellLifeSentence(
        [v({ capacity: '13400000000', occupiedCapacity: '13300000000', isLive: true })],
        'token'
      )
    ).toBe('The token’s cell holds 134 CKB, 133 of it occupied by its bytes.');
  });

  it('says what a consumed last cell held when it was consumed, nothing in the present', () => {
    const a = cell(307, 53);
    const b = cell(320, 66, { consumedByTx: `0x${'56'.repeat(32)}`, consumedAtBlock: 101 });
    expect(cellLifeSentence([a, b], 'object')).toBe(
      'The object’s last cell held 320 CKB when it was consumed. At minting it held 307 CKB, its lock 53\u00a0bytes; after 1\u00a0move its lock was 66\u00a0bytes.'
    );
    expect(cellLifeSentence([a, b], 'object')).not.toMatch(/holds| is /);
  });

  it('says a cell minted then melted held its CKB when it was consumed', () => {
    expect(cellLifeSentence([cell(320, 53)], 'object')).toBe(
      'The object’s cell held 320 CKB when it was consumed.'
    );
    expect(
      cellLifeSentence(
        [v({ capacity: '13400000000', occupiedCapacity: '13300000000', isLive: false })],
        'token'
      )
    ).toBe('The token’s cell held 134 CKB, 133 of it occupied by its bytes, when it was consumed.');
  });

  it('says two growths over two moves as what minting held and what now holds, no difference', () => {
    const versions = [cell(307, 53), cell(313, 59), cell(320, 66, { isLive: true })];
    expect(cellLifeSentence(versions, 'object')).toBe(
      'The object’s cell holds 320 CKB. At minting it held 307 CKB, its lock 53\u00a0bytes; after 2\u00a0moves its lock is 66\u00a0bytes.'
    );
    // no figure the page subtracted: neither 13 CKB nor 13 bytes, nor either step's 6 or 7
    expect(cellLifeSentence(versions, 'object')).not.toMatch(/\b(13|6|7)\b/);
  });

  it('says moves that changed no part of the cell by their count alone', () => {
    const versions = [cell(320, 53), cell(320, 53), cell(320, 53, { isLive: true })];
    expect(cellLifeSentence(versions, 'object')).toBe(
      'The object’s cell holds 320 CKB. At minting it held 320 CKB; it has moved 2\u00a0times since.'
    );
    const gone = [cell(320, 53), cell(320, 53)];
    expect(cellLifeSentence(gone, 'token')).toBe(
      'The token’s last cell held 320 CKB when it was consumed. At issue it held 320 CKB; it moved 1\u00a0time before its last cell was consumed.'
    );
  });
});

it('pairs every composition tier with one sentence and keeps Live one style', () => {
  expect(Object.keys(COMPOSITION_SENTENCES)).toHaveLength(6);
  const { rerender } = render(<LiveBadge live gone="Melted" />);
  expect(screen.getByText('Live')).toHaveClass('text-jade');
  rerender(<LiveBadge live={false} gone="Melted" />);
  // Re-pinned on purpose (readability R8, lane p-assets): a gone object's state reads in the
  // reading ink on a border that shows (it was the quiet ink on a border near the ground)
  expect(screen.getByText('Melted')).toHaveClass('text-text', 'border-text-dim');
  expect(screen.getByText('Melted').className).not.toMatch(/text-text-dim|border-base-border/);
});

it('lists readable traits only and reports hover', () => {
  const onActive = vi.fn();
  render(
    <ObjectTraits
      traits={[
        { name: 'prev.bg', value: 'btcfs://aa' },
        { name: 'Color', value: 'Nature Green' },
        { name: 'IMAGE', value: '<svg xmlns="x"></svg>' },
      ]}
      colors={new Map([['Color', 'var(--c-aqua)']])}
      active={null}
      onActive={onActive}
    />
  );
  const list = screen.getByLabelText('Traits');
  expect(list.querySelectorAll('[data-trait]')).toHaveLength(1);
  fireEvent.mouseEnter(list.querySelector('[data-trait="Color"]')!);
  expect(onActive).toHaveBeenCalledWith('Color');
});

// F2 objects (OB-14): tier 2 shows what the decoder produced, not only the DNA bytes.
import { ObjectGeneticsEvidence } from '@/components/asset/genetics';
import { nervape3xGenetics } from '@/__tests__/fixtures/objects/nervape3x';
it('lists the decoder output beside the DNA evidence (OB-14)', () => {
  render(
    <ObjectGeneticsEvidence
      evidence={nervape3xGenetics}
      subject="Nervape 3x"
      decoded={{
        status: 'decoded',
        sporeId: '0x51c7',
        contentType: 'dob/0',
        dnaHex: nervape3xGenetics.dnaHex,
        traits: nervape3xGenetics.cachedVmTraits.traits,
        media: [
          {
            mediaType: 'application/json',
            size: 379,
            hash: '968c587811f54682910668c41573198061a8fc923457eb63ba8559f0e97af74e',
            step: 0,
            url: '/spore/objects/0x51c7/media/968c5878',
          },
        ],
        render: null,
        issues: [],
      }}
    />
  );
  const out = screen.getByRole('list', { name: 'Decoder output' });
  expect(out).toHaveTextContent('application/json · 379 bytes · step 1');
  expect(screen.getByRole('link', { name: /Open the decoded file/ })).toHaveAttribute(
    'href',
    expect.stringContaining('/spore/objects/0x51c7/media/968c5878')
  );
});
// The owner's report (2026-11-18): the page of 0x707532f6…c7ba, a DOB/1 object whose IMAGE trait
// holds its SVG, threw "shorten: an empty hash". The decoder's stored outputs are listed with
// their bytes, step and sha256; the picture /render draws from the traits has no stored bytes, so
// its row says what it is and gives neither a byte count nor a sha256.
import { within } from '@/__tests__/utils/test-utils';
import {
  dob1SvgDecoded,
  DOB1_SVG_ID,
  DOB1_SVG_STEP0_HASH,
  DOB1_SVG_STEP1_HASH,
} from '@/__tests__/fixtures/objects/dob1-svg-trait';
it('lists a DOB/1 object’s stored outputs with bytes and sha256, its drawn picture with neither', () => {
  render(
    <ObjectGeneticsEvidence
      evidence={nervape3xGenetics}
      subject="Its object"
      decoded={dob1SvgDecoded()}
    />
  );
  const rows = within(screen.getByRole('list', { name: 'Decoder output' })).getAllByRole(
    'listitem'
  );
  expect(rows).toHaveLength(3);
  expect(rows[0]).toHaveTextContent(
    'application/json · 456 bytes · step 1 sha256 d0111ca8e5…e0d606 Open the decoded file ↗'
  );
  expect(rows[1]).toHaveTextContent(
    'application/json · 22,953 bytes · step 2 sha256 7a8245b77f…af37b0 Open the decoded file ↗'
  );
  expect(within(rows[0]).getByRole('link')).toHaveAttribute(
    'href',
    expect.stringMatching(new RegExp(`/spore/objects/${DOB1_SVG_ID}/media/${DOB1_SVG_STEP0_HASH}$`))
  );
  expect(within(rows[1]).getByRole('link')).toHaveAttribute(
    'href',
    expect.stringMatching(new RegExp(`/spore/objects/${DOB1_SVG_ID}/media/${DOB1_SVG_STEP1_HASH}$`))
  );
  expect(rows[2]).toHaveTextContent(
    'image/svg+xml · drawn from the traits on request, not a stored file Open the picture ↗'
  );
  expect(rows[2]).not.toHaveTextContent(/byte|sha256|step/);
  expect(within(rows[2]).getByRole('link', { name: 'Open the picture ↗' })).toHaveAttribute(
    'href',
    expect.stringMatching(new RegExp(`/spore/objects/${DOB1_SVG_ID}/render$`))
  );
});

// Lane P2's third round (item 4): a label and its value are read together. A trait's value
// stood at its row's right edge, "relief" at the left of a 780 px cell and "NERVAPE 3X" 600 px
// away at 1280; it follows its label at one gap (12 px), left-aligned, the label column as wide as
// the grid's longest label: Nervape 3x's readable traits are relief, Color, Serial and Note, the
// longest six characters at the page's 14 px (8.4 px a glyph), 50.4 px, and the swatch's 8 and its
// 8 px margin, 66.4: 67 px.
it('sets each trait’s value at one gap after its label, the label column the longest label', () => {
  render(
    <ObjectTraits
      traits={nervape3xGenetics.cachedVmTraits.traits}
      colors={new Map()}
      active={null}
      onActive={() => {}}
    />
  );
  const rows = [...document.querySelectorAll<HTMLElement>('[data-trait]')];
  expect(rows.map((r) => r.dataset.trait)).toEqual(['relief', 'Color', 'Serial', 'Note']);
  for (const row of rows) {
    expect(row).not.toHaveClass('justify-between');
    expect(row.style.gridTemplateColumns).toBe('67px minmax(0, 1fr)');
    expect(row).toHaveClass('gap-x-3');
    expect(row.querySelector('dd')).not.toHaveClass('text-right');
  }
});

// Lane P2's fifth round (NOTE 5): a trait's name is the collection creator's text, measured by the
// bound for it (a Latin glyph 0.6 em, any other grapheme 1.25 em), not the Latin advance, and the
// label column is capped at half of the narrowest row a window gives the traits, 288 px at 320
// less the 12 px gap, 138 px: a longer name breaks within its column and its value keeps the rest.
describe('the trait label column holds a creator’s name', () => {
  const traitsOf = (names: string[]) => names.map((name) => ({ name, value: 'Red' }));
  const column = () =>
    document.querySelector<HTMLElement>('[data-trait]')!.style.gridTemplateColumns;
  it('measures a CJK name at its bound: 4 × 1.25 × 14 = 70 px and the swatch’s 16', () => {
    render(
      <ObjectTraits
        traits={traitsOf(['背景颜色', 'Color'])}
        colors={new Map()}
        active={null}
        onActive={() => {}}
      />
    );
    expect(column()).toBe('86px minmax(0, 1fr)');
  });

  it('caps a long name’s column at 138 px, where its words break', () => {
    render(
      <ObjectTraits
        traits={traitsOf(['The background colour of this particular ape', 'Color'])}
        colors={new Map()}
        active={null}
        onActive={() => {}}
      />
    );
    expect(column()).toBe('138px minmax(0, 1fr)');
    const label = document.querySelector('[data-trait] dt')!;
    expect(label).toHaveClass('min-w-0', '[overflow-wrap:anywhere]');
  });
});

// Lane P2's third round (item 15; review-p-logic-pages NOTE-2): "throughout" only where every
// version's lock is the first's (code hash, hash type and args); it compared the first and the
// last, so a cell that went A → B → A read "Lock: … throughout." Else the footnote says where the
// lock changed: each lock the cell sat under, in order, from the day it went under it.
describe('the lock footnote says where the lock changed', () => {
  const DAY = 86_400_000;
  const JOY_ARGS = `0x${'0a'.repeat(22)}`;
  const DEFAULT_ARGS = `0x${'01'.repeat(20)}`;
  const under = (
    lock: { codeHash: string; hashType: 'type' | 'data'; args: string },
    lockName: string,
    day: number,
    isLive: boolean
  ) =>
    v({
      timestamp: String(Date.parse('2026-01-01T00:00:00+08:00') + day * DAY),
      isLive,
      lock,
      lockName,
      ownerAddress: ckbAddress(lock),
      anatomy: {
        capacityBytes: 8,
        lockBytes: 33 + (lock.args.length - 2) / 2,
        typeBytes: 0,
        dataBytes: 0,
      },
    });
  const DEFAULT = {
    codeHash: DEFAULT_LOCK_CODE_HASH,
    hashType: 'type' as const,
    args: DEFAULT_ARGS,
  };
  const JOY = { codeHash: JOYID_CODE_HASH, hashType: 'type' as const, args: JOY_ARGS };

  it('says the lock it went back to, never "throughout"', () => {
    const versions = [
      under(DEFAULT, 'Default Lock', 0, false),
      under(JOY, 'JoyID', 3, false),
      under(DEFAULT, 'Default Lock', 9, true),
    ];
    expect(lockFootnote(versions, 'object')).toBe(
      'Lock: Default Lock with 20-byte args at minting → JoyID with 22-byte args from 2026-01-04 → Default Lock with 20-byte args since 2026-01-10. Type and data never changed.'
    );
  });

  it('says another holder under the same lock as other args', () => {
    const other = { ...DEFAULT, args: `0x${'02'.repeat(20)}` };
    const versions = [
      under(DEFAULT, 'Default Lock', 0, false),
      under(other, 'Default Lock', 2, false),
    ];
    expect(lockFootnote(versions, 'token')).toBe(
      'Lock: Default Lock with 20-byte args at issue → Default Lock with other 20-byte args from 2026-01-03, last. Type and data never changed.'
    );
  });

  it('reads a hash type that changed under the same code hash and args', () => {
    const byData = { ...DEFAULT, hashType: 'data' as const };
    const versions = [
      under(DEFAULT, 'Default Lock', 0, false),
      under(byData, 'Default Lock', 1, true),
    ];
    expect(lockFootnote(versions, 'object')).toBe(
      'Lock: Default Lock with 20-byte args at minting → lock 0x9bd7e06f…a3cce8 by data with 20-byte args since 2026-01-02. Type and data never changed.'
    );
  });

  it('says "throughout" where every version is under the first’s lock', () => {
    const versions = [
      under(DEFAULT, 'Default Lock', 0, false),
      under(DEFAULT, 'Default Lock', 5, true),
    ];
    expect(lockFootnote(versions, 'object')).toBe(
      'Lock: Default Lock throughout. Type and data never changed.'
    );
  });
});
