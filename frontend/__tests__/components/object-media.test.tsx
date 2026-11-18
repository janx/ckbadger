import { MemoryRouter, Route, Routes } from 'react-router-dom';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ObjectMedia } from '@/components/asset/object-media';
import type { SporeDetail } from '@/lib/api';
import { bitcoinTxHref, omigaDobHref, parseBtcfsUri } from '@/lib/btcfs';
import { fullText } from '@/__tests__/utils/full-text';

// A Bitcoin txid shaped like the Nervape 3x one (0bfb…5719); the middle digits are a fixture.
const txid = '0bfb6dd6b0ac0fa8d8a9a0c8ad66d33c63cb34e08cf8ea76b8b6a1fbd2f15719';
const object: SporeDetail = {
  sporeId: '0x' + '12'.repeat(32),
  txHash: '0x' + '23'.repeat(32),
  outputIndex: 0,
  clusterId: null,
  contentType: 'image/png',
  contentSize: 1,
  ownerLockHash: null,
  isLive: true,
  createdAtBlock: 1,
  ownedCapacity: '6100000000',
  ownedKnowledge: '6100000000',
  typeScriptHash: '0x303e7b5e16db979e24e79690a8f3a3436cffd00e8c60b40e067db097136cc998',
  mediaProfile: {
    tier: 'btc_ckb',
    sources: [
      {
        uri: `btcfs://${txid}i0`,
        scheme: 'btcfs',
        sourceLocation: 'DOB trait',
        dependencyTier: 'btc_ckb',
      },
    ],
    issues: [],
  },
};
it('presents Bitcoin media as a copper link plate without fetching an image', () => {
  const { container } = render(
    <ObjectMedia
      object={object}
      contentHex="0x00"
      decoded={{
        status: 'decoded',
        sporeId: object.sporeId,
        contentType: 'dob/0',
        dnaHex: '0x00',
        traits: [],
        issues: [],
        media: [
          {
            mediaType: 'image/png',
            size: 1,
            hash: 'x',
            step: 0,
            url: 'https://external.example/image',
          },
        ],
        render: null,
      }}
    />
  );
  // OB-2: browsers cannot open btcfs://; the inscription is read on its Bitcoin transaction.
  const inscription = screen.getByRole('link', {
    name: 'Bitcoin tx 0bfb6dd6b0…f15719 · inscription i0 ↗',
  });
  expect(inscription).toHaveAttribute('href', `https://mempool.space/tx/${txid}`);
  expect(inscription).toHaveAttribute('target', '_blank');
  expect(inscription).toHaveAttribute('rel', 'noopener noreferrer');
  expect(screen.getByText(/CKBadger reads CKB only/)).toBeInTheDocument();
  // Lane U follow-up (user: "'Omiga' should be called 'Image View' or so, and far more visible"):
  // ckbadger does not render the picture, so the way to see it sits where the eye looks for it —
  // a filled jade button centred in the media frame, its off-site source said beneath.
  const view = screen.getByRole('link', { name: 'View image ↗' });
  expect(view).toHaveAttribute(
    'href',
    'https://omiga.io/info/dobs/0x303e7b5e16db979e24e79690a8f3a3436cffd00e8c60b40e067db097136cc998'
  );
  expectViewImageButton(view);
  expect(view.closest('figure')).toBe(
    screen.getByText(/Its image lives on Bitcoin/).closest('figure')
  );
  expect(screen.queryByRole('link', { name: /View on Omiga/ })).toBeNull();
  expect(container.querySelector('img')).toBeNull();
  expect(screen.getByText(/Its image lives on Bitcoin/)).toHaveClass('text-copper');
});
/** The one "View image" button: filled jade, 36 px, 13 px text, external, its source beneath. */
function expectViewImageButton(view: HTMLElement) {
  expect(view).toHaveClass('bg-jade', 'text-base-bg', 'h-9', 'text-body');
  expect(view).toHaveAttribute('target', '_blank');
  expect(view).toHaveAttribute('rel', 'noopener noreferrer');
  expect(view).toHaveAttribute('title', "Opens the object's picture on Omiga in a new tab");
  const block = view.closest('[data-role="view-image"]')!;
  expect(block).toHaveClass('items-center');
  expect(block).toHaveTextContent(/^View image ↗rendered off-site by Omiga$/);
  expect(view.nextElementSibling).toHaveTextContent('rendered off-site by Omiga');
  expect(view.nextElementSibling).toHaveClass('text-meta', 'text-text-dim');
}
it('centres the button in a square placeholder with the cube mark when nothing renders here', () => {
  render(
    <ObjectMedia object={{ ...object, mediaProfile: undefined } as never} contentHex={null} />
  );
  const frame = screen.getByRole('figure', { name: 'No picture shown here' });
  expect(frame).toHaveClass('aspect-square', 'justify-center', 'items-center');
  expect(frame.querySelector('[data-class="objects"]')).not.toBeNull();
  expect(frame).toHaveTextContent('No renderable media available.');
  const view = screen.getByRole('link', { name: 'View image ↗' });
  expect(frame).toContainElement(view);
  expectViewImageButton(view);
});
it('keeps the button under a picture that renders here', () => {
  const { container } = render(
    <ObjectMedia
      object={{ ...object, mediaProfile: { tier: 'pure_ckb', sources: [], issues: [] } }}
      contentHex="0x89504e47"
    />
  );
  expect(container.querySelector('img')).not.toBeNull();
  const view = screen.getByRole('link', { name: 'View image ↗' });
  expectViewImageButton(view);
  expect(container.querySelector('img')!.compareDocumentPosition(view)).toBe(
    Node.DOCUMENT_POSITION_FOLLOWING
  );
});
it('leaves the Omiga link out when asked (omigaLink={false})', () => {
  render(
    <ObjectMedia
      object={{ ...object, mediaProfile: undefined } as never}
      contentHex={null}
      omigaLink={false}
    />
  );
  expect(screen.queryByRole('link', { name: /Omiga/ })).toBeNull();
});
it('does not label pending composition as pure CKB', () => {
  render(
    <ObjectMedia
      object={{ ...object, mediaProfile: { tier: 'decode_pending', sources: [], issues: [] } }}
      contentHex={null}
    />
  );
  expect(screen.getByText(/awaiting DOB execution/)).toBeInTheDocument();
  expect(screen.queryByText('Pure CKB')).not.toBeInTheDocument();
});
it('parses BTCFS inscription URIs strictly (OB-2)', () => {
  expect(parseBtcfsUri(`btcfs://${txid}i0`)).toEqual({ txid, index: 0 });
  expect(parseBtcfsUri(`btcfs://${txid.toUpperCase()}i12`)).toEqual({ txid, index: 12 });
  for (const uri of [
    'btcfs://abc123i0',
    `btcfs://${txid}`,
    `btcfs://${txid}i`,
    `btcfs://${txid}i01`,
    `btcfs://${txid}i-1`,
    `btcfs://0x${txid}i0`,
    `ord://${txid}i0`,
    `btcfs://${txid}i0/extra`,
    `btcfs://${txid}i99999999999999999999`,
  ])
    expect(parseBtcfsUri(uri)).toBeNull();
  expect(bitcoinTxHref(txid, 'mainnet')).toBe(`https://mempool.space/tx/${txid}`);
  expect(bitcoinTxHref(txid, 'testnet')).toBeNull();
  expect(() => bitcoinTxHref(`0x${txid}`, 'mainnet')).toThrow('Invalid Bitcoin txid');
  // The type script hash is our API's own computed value: a bad one is our invariant violation.
  expect(() => omigaDobHref('0x1234', 'mainnet')).toThrow('Invalid spore type script hash');
});
it('shows a malformed minted BTCFS reference as plain text, never a link or a crash', () => {
  // Anyone can mint spore content; a bad URI there must not take the object page down.
  const malformed = 'btcfs://abc123i0';
  render(
    <ObjectMedia
      object={{
        ...object,
        mediaProfile: {
          tier: 'btc_ckb',
          sources: [
            { uri: malformed, scheme: 'btcfs', sourceLocation: 'DNA', dependencyTier: 'btc_ckb' },
            {
              uri: `btcfs://${txid}i1`,
              scheme: 'btcfs',
              sourceLocation: 'DNA',
              dependencyTier: 'btc_ckb',
            },
          ],
          issues: [],
        },
      }}
      contentHex={null}
    />
  );
  expect(screen.getAllByText(malformed).length).toBeGreaterThan(0);
  expect(screen.getByText('Not a recognisable btcfs inscription reference.')).toBeInTheDocument();
  expect(screen.queryByRole('link', { name: /abc123/ })).not.toBeInTheDocument();
  expect(
    screen.getByRole('link', { name: 'Bitcoin tx 0bfb6dd6b0…f15719 · inscription i1 ↗' })
  ).toHaveAttribute('href', `https://mempool.space/tx/${txid}`);
});
// PB-15 (UI review 2026-11-14): with many content references the box printed every raw btcfs://
// URI, centred, before "View image ↗", and a phone's first screen ended inside the hex. The box
// shows the first reference and then the action, right under the title; the further references
// move into the "Media dependencies" disclosure that follows the box, each with its inscription
// reading (link or note), so nothing is dropped.
it('shows the first reference and the action under the title, the rest in the disclosure (PB-15)', () => {
  const later = `btcfs://${'ab'.repeat(32)}i3`;
  const malformed = 'btcfs://abc123i0';
  render(
    <ObjectMedia
      object={{
        ...object,
        mediaProfile: {
          tier: 'btc_ckb',
          sources: [
            ...object.mediaProfile!.sources,
            { uri: later, scheme: 'btcfs', sourceLocation: 'DNA', dependencyTier: 'btc_ckb' },
            { uri: malformed, scheme: 'btcfs', sourceLocation: 'DNA', dependencyTier: 'btc_ckb' },
          ],
          issues: [],
        },
      }}
      contentHex={null}
    />
  );
  const title = screen.getByText('Its image lives on Bitcoin');
  const box = title.closest('figure')!;
  const follows = (a: Node, b: Node) =>
    Boolean(a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING);
  // title → the first reference (its URI, then its inscription) → the action
  const uri = within(box).getByText(`btcfs://${txid}i0`);
  const inscription = within(box).getByRole('link', {
    // U-09 (re-pinned): the txid at the row size (it was 4…4)
    name: 'Bitcoin tx 0bfb6dd6b0…f15719 · inscription i0 ↗',
  });
  const view = within(box).getByRole('link', { name: 'View image ↗' });
  expect(follows(title, uri)).toBe(true);
  expect(follows(uri, inscription)).toBe(true);
  expect(follows(inscription, view)).toBe(true);
  // the further references are not in the box
  expect(box).not.toHaveTextContent(later);
  expect(box).not.toHaveTextContent(malformed);
  expect(within(box).queryByText('Not a recognisable btcfs inscription reference.')).toBeNull();
  // they are in the disclosure that follows it, whole: URI and reading
  const details = screen.getByText(fullText('Media dependencies · 3 sources')).closest('details')!;
  expect(follows(box, details)).toBe(true);
  expect(within(details).getByText(later)).toBeInTheDocument();
  expect(
    within(details).getByRole('link', { name: 'Bitcoin tx ababababab…ababab · inscription i3 ↗' })
  ).toHaveAttribute('href', `https://mempool.space/tx/${'ab'.repeat(32)}`);
  expect(within(details).getByText(malformed)).toBeInTheDocument();
  expect(
    within(details).getByText('Not a recognisable btcfs inscription reference.')
  ).toBeInTheDocument();
  // the first is read once, in the box
  expect(screen.getAllByRole('link', { name: /inscription i0/ })).toHaveLength(1);
});
it('reads the btcfs scheme case-insensitively, as the indexer classifies it (#8)', () => {
  expect(parseBtcfsUri(`BTCFS://${txid.toUpperCase()}I3`)).toEqual({ txid, index: 3 });
  expect(bitcoinTxHref(txid, 'mainnet')).toBe(`https://mempool.space/tx/${txid}`);
  expect(omigaDobHref(object.typeScriptHash, 'testnet')).toBe(
    `https://test.omiga.io/info/dobs/${object.typeScriptHash}`
  );
});
it('follows the page network: no mainnet links from a testnet object (#8)', () => {
  render(
    <MemoryRouter initialEntries={['/testnet/objects/x']}>
      <Routes>
        <Route path="/:network/*" element={<ObjectMedia object={object} contentHex={null} />} />
      </Routes>
    </MemoryRouter>
  );
  // Which Bitcoin network testnet DOB inscriptions name is not established: no explorer link.
  expect(screen.getByRole('region', { name: 'Object media' })).toHaveTextContent(
    'Bitcoin tx 0bfb6dd6b0…f15719 · inscription i0'
  );
  expect(screen.queryByRole('link', { name: /Bitcoin tx/ })).not.toBeInTheDocument();
  expect(
    screen.getByText(/Bitcoin network of testnet inscriptions is not established/)
  ).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'View image ↗' })).toHaveAttribute(
    'href',
    `https://test.omiga.io/info/dobs/${object.typeScriptHash}`
  );
});

// B3 round 3 (the lead's sweep for plain words cut in the middle): the inscription reading took
// break-all, so "inscription i0" was set "inscript / ion i0" in the media box at 390 and 1280, and
// the notice "Not a recognisable btcfs inscription reference." inherited it from its list item.
// Only a raw URI line breaks anywhere; a label or a sentence of words never takes break-all, own
// or inherited, and the reading's two units hold: "Bitcoin tx" with its id, "inscription i<n>".
it('keeps break-all to the raw URI lines; the readings and the notice break between words', () => {
  const malformed = 'btcfs://abc123i0';
  const { container } = render(
    <ObjectMedia
      object={{
        ...object,
        mediaProfile: {
          tier: 'btc_ckb',
          sources: [
            ...object.mediaProfile!.sources,
            { uri: malformed, scheme: 'btcfs', sourceLocation: 'DNA', dependencyTier: 'btc_ckb' },
          ],
          issues: [],
        },
      }}
      contentHex={null}
    />
  );
  const breaksAll = (el: Element) => Boolean(el.closest('.break-all'));
  // the raw URIs keep it
  const uris = [...screen.getAllByText(`btcfs://${txid}i0`), screen.getByText(malformed)];
  for (const uri of uris) expect(uri).toHaveClass('break-all');
  // the reading and the notice never take it, their own or a parent's
  const reading = screen.getByRole('link', { name: /inscription i0 ↗$/ });
  expect(breaksAll(reading)).toBe(false);
  const notice = screen.getByText('Not a recognisable btcfs inscription reference.');
  expect(breaksAll(notice)).toBe(false);
  // the words of each dependency's head line neither
  for (const head of screen.getAllByText(/^btcfs · /)) expect(breaksAll(head)).toBe(false);
  // the reading's units: "Bitcoin tx" with its id, and "inscription i0", each one NoBreak
  const units = Array.from(reading.querySelectorAll(':scope > [data-nobreak]')).map(
    (u) => u.textContent
  );
  expect(units).toEqual(['Bitcoin tx 0bfb6dd6b0…f15719', 'inscription i0']);
  expect(reading).toHaveTextContent(/^Bitcoin tx 0bfb6dd6b0…f15719 · inscription i0 ↗$/);
  expect(container.querySelector('figure')).not.toBeNull();
});

// B3 round 4, task 2: a value of btcfs inscriptions joined by ';' (the 秦晖 collection's Source,
// which traitDisplay reads as "5 inscriptions") was "Not a recognisable btcfs inscription
// reference." here. It is read through the one path that decides what such a list is, one reading
// per inscription, each the existing label and explorer link; a list with an element that is not
// an inscription keeps the notice.
it('reads a ;-list of inscriptions one reading each, and keeps the notice for a broken list', () => {
  const second = 'cd'.repeat(32);
  const list = `btcfs://${txid}i0;btcfs://${second}i2`;
  const broken = `btcfs://${txid}i0;btcfs://not-one`;
  render(
    <ObjectMedia
      object={{
        ...object,
        mediaProfile: {
          tier: 'btc_ckb',
          sources: [
            { uri: list, scheme: 'btcfs', sourceLocation: 'DOB trait', dependencyTier: 'btc_ckb' },
            { uri: broken, scheme: 'btcfs', sourceLocation: 'DNA', dependencyTier: 'btc_ckb' },
          ],
          issues: [],
        },
      }}
      contentHex={null}
    />
  );
  const box = screen.getByText('Its image lives on Bitcoin').closest('figure')!;
  expect(
    within(box).getByRole('link', { name: 'Bitcoin tx 0bfb6dd6b0…f15719 · inscription i0 ↗' })
  ).toHaveAttribute('href', `https://mempool.space/tx/${txid}`);
  expect(
    within(box).getByRole('link', { name: 'Bitcoin tx cdcdcdcdcd…cdcdcd · inscription i2 ↗' })
  ).toHaveAttribute('href', `https://mempool.space/tx/${second}`);
  expect(within(box).queryByText('Not a recognisable btcfs inscription reference.')).toBeNull();
  const details = screen.getByText(fullText('Media dependencies · 2 sources')).closest('details')!;
  expect(
    within(details).getByText('Not a recognisable btcfs inscription reference.')
  ).toBeInTheDocument();
});
