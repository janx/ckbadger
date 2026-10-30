// F2 group `objects` (round 2): the shared hero pieces of the Object and Mnft boards.
import { render, screen, fireEvent } from '@/__tests__/utils/test-utils';
import {
  COMPOSITION_SENTENCES,
  LiveBadge,
  ObjectTraits,
  cellLifeSentence,
  itemLifeSentence,
  lockPhrase,
  spelledDuration,
} from '@/components/asset/object-hero';
import { sporeCellVersionsProducer } from '@/__tests__/msw/fixtures/item-producer';
import type { ItemCellVersion } from '@/lib/api';

const v = (over: Partial<ItemCellVersion>): ItemCellVersion => ({
  ...sporeCellVersionsProducer.data[0],
  ...over,
});

it('spells durations and lock phrases for prose', () => {
  expect(spelledDuration(30_000)).toBe('30 seconds');
  expect(spelledDuration(60_000)).toBe('1 minute');
  expect(spelledDuration(2 * 86_400_000)).toBe('2 days');
  expect(spelledDuration(800 * 86_400_000)).toBe('2 years');
  expect(() => spelledDuration(-1)).toThrow('invalid duration');
  expect(lockPhrase('Nervape Shadow Lock')).toBe('a Nervape Shadow Lock');
  expect(lockPhrase('secp256k1')).toBe('a secp256k1 lock');
  expect(lockPhrase('Omnilock')).toBe('an Omnilock');
  expect(lockPhrase(null)).toBe('an unregistered lock');
});

it('tells an item’s life from its cell versions', () => {
  const now = 10 * 86_400_000;
  const minted = v({ timestamp: '0', lockName: 'secp256k1', isLive: false });
  const moved = v({ timestamp: '30000', lockName: 'Nervape Shadow Lock', isLive: true });
  expect(itemLifeSentence([{ ...moved, timestamp: '0' }], now)).toBe(
    'Minted 10 days ago to a Nervape Shadow Lock, where it sits now.'
  );
  expect(itemLifeSentence([minted, moved], now)).toBe(
    'Minted 10 days ago to a secp256k1 lock, then moved 30 seconds later to a Nervape Shadow Lock, where it sits now.'
  );
  expect(
    itemLifeSentence([minted, { ...moved, isLive: false, consumedAtBlock: 12 }], now, 'Issued', 'X')
  ).toBe(
    'Issued by X 10 days ago to a secp256k1 lock, moved 1 time; its last cell was consumed at block 12.'
  );
  expect(() => itemLifeSentence([], now)).toThrow('at least one cell version');
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
    'The object’s cell holds 320 CKB. It held 307 at minting; the lock grew by 13 bytes, so the cell grew by 13 CKB when it moved.'
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
  expect(screen.getByText('Melted')).toHaveClass('text-text-dim');
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
      colors={new Map([['Color', '#68ccf0']])}
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
            role: null,
            size: 379,
            hash: '968c587811f54682910668c41573198061a8fc923457eb63ba8559f0e97af74e',
            step: 0,
            url: '/spore/objects/0x51c7/media/968c5878',
          },
        ],
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
