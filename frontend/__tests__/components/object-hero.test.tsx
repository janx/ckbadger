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
import { ckbAddress } from '@/__tests__/utils/ckb-address';
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
    // re-keyed (lane rd/api-lock-names, fix round ruling 2): the API names the code a pair runs,
    // and a lock on the default lock's code hash by data runs none, so it sends no name
    lockName: null,
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

it('explains what changed in the cell between minting and now', () => {
  const a = v({
    capacity: '30700000000',
    occupiedCapacity: '30700000000',
    anatomy: { capacityBytes: 8, lockBytes: 53, typeBytes: 65, dataBytes: 181 },
  });
  const b = v({
    capacity: '32000000000',
    occupiedCapacity: '32000000000',
    anatomy: { capacityBytes: 8, lockBytes: 66, typeBytes: 65, dataBytes: 181 },
  });
  expect(cellLifeSentence([a, b], 'object')).toBe(
    'The object’s cell holds 320 CKB. It held 307 at minting; the lock grew by 13\u00a0bytes, so the cell grew by 13 CKB when it moved.'
  );
  expect(
    cellLifeSentence([v({ capacity: '13400000000', occupiedCapacity: '13300000000' })], 'token')
  ).toBe('The token’s cell holds 134 CKB, 133 of it occupied by its bytes.');
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
