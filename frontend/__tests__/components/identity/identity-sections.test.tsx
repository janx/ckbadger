import { render, screen, within } from '@/__tests__/utils/test-utils';
import { IdentityRecords } from '@/components/identity/identity-records';
import { DotcellRoles } from '@/components/identity/dotcell-roles';
import { DidDocumentReadout, AmongDidCkb } from '@/components/identity/did-readout';
import type { ItemProtocol } from '@/lib/api';
import {
  didProtocolProducer,
  dotcellProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { expectPrompt, promptNote } from '@/__tests__/utils/prompt-heading';

const rec = (key: string, valueUtf8: string | null, valueHex = '0x00') => ({
  key,
  valueUtf8,
  valueHex,
  ttl: 300,
  address: null as string | null,
});

describe('IdentityRecords (ID-DC1 / ID-DC5)', () => {
  it('names addresses by coin, lays out the profile and gives the accent a swatch', () => {
    render(
      <IdentityRecords
        subject="apt.cell"
        records={[
          rec('address.0', 'bc1qxyz'),
          rec('address.60', '0xabc'),
          { ...rec('address.309', 'ckb1qzda0'), address: 'ckb1qzda0' },
          rec('profile.name', 'Albert'),
          rec('profile.accent', '#476de1'),
          rec('profile.avatar', null, '0x' + 'ab'.repeat(20184)),
        ]}
      />
    );
    const section = screen.getByRole('region', { name: 'What apt.cell points to' });
    const bitcoin = within(section).getByText('Bitcoin');
    expect(bitcoin.closest('div')).toHaveTextContent('address.0 · coin type 0');
    expect(section).toHaveTextContent('bc1qxyz');
    expect(within(section).getByText('Ethereum')).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: 'ckb1qzda0' })).toHaveAttribute(
      'href',
      '/mainnet/address/ckb1qzda0'
    );
    expect(within(section).getByText('Albert')).toBeInTheDocument();
    const swatch = section.querySelector('[data-accent-swatch]') as HTMLElement;
    expect(swatch.style.backgroundColor).toBe('rgb(71, 109, 225)');
    expect(section).toHaveTextContent('20,184 bytes, not text');
    expect(section).not.toHaveTextContent('abababab');
    expect(section).toHaveTextContent('6 records · each cached for 300 s');
    // Phase 5 (A5): the records, run on the name and their count; no note beside them.
    const heading = within(section).getByRole('heading', { name: 'Records apt.cell · 6' });
    expectPrompt(heading, 'Records', 'apt.cell · 6');
    expect(heading.parentElement).toBe(section);
    expect(section).not.toHaveTextContent('its records');
  });

  it("marks a CKB address that is the manager's own lock (ID-DC5)", () => {
    render(
      <IdentityRecords
        subject="apt.cell"
        records={[
          { ...rec('address.309', 'ckb1mgr'), address: 'ckb1mgr', addressLockHash: '0x1e' },
          { ...rec('address.0', 'bc1q'), addressLockHash: null },
        ]}
        parties={{
          owner: { lockHash: '0x69', scriptName: 'Quantum-Resistant Lock' },
          manager: { lockHash: '0x1e', scriptName: 'JoyID' },
        }}
      />
    );
    // Each address row is its own grid row: the note belongs to the CKB row only.
    expect(screen.getByText('CKB').closest('div')).toHaveTextContent(
      "the manager's own JoyID wallet"
    );
    expect(screen.getByText('Bitcoin').closest('div')).not.toHaveTextContent('own');
  });

  it('says plainly when no records are set', () => {
    render(<IdentityRecords subject="valar.bit" records={[]} />);
    expect(screen.getByRole('region', { name: 'What valar.bit points to' })).toHaveTextContent(
      'No records are set.'
    );
    expect(screen.getByRole('heading', { name: 'Records valar.bit · 0' })).toBeInTheDocument();
  });
});

function dotcellHistory(): Extract<ItemProtocol, { kind: 'dotcell' }> {
  const p = structuredClone(dotcellProtocolProducer) as Extract<ItemProtocol, { kind: 'dotcell' }>;
  const v0 = p.versions[0];
  const party = (prefix: string, address: string | null, scriptName: string | null) => ({
    hashPrefix: '0x' + prefix.repeat(20),
    lockHash: address ? '0x' + prefix.repeat(32) : null,
    address,
    scriptName,
  });
  const at = (ts: string, owner: ReturnType<typeof party>, manager: ReturnType<typeof party>) => {
    const v = structuredClone(v0);
    v.cell.timestamp = String(Date.parse(ts));
    v.cell.txHash = '0x' + String(Date.parse(ts)).padStart(64, '0');
    v.decoded.owner = owner;
    v.decoded.manager = manager;
    return v;
  };
  const registrar = party('57', 'ckb1registrar', 'secp256k1');
  const joy = party('ac', 'ckb1joyid', 'JoyID');
  const qr = party('69', null, null);
  const manager = party('1e', 'ckb1manager', 'JoyID');
  p.versions = [
    at('2026-09-20T05:05:00Z', registrar, registrar),
    at('2026-09-21T05:05:00Z', joy, joy),
    at('2026-09-23T05:05:00Z', qr, manager),
    at('2026-10-01T05:05:00Z', qr, manager),
  ];
  return p;
}

// J13 (ID-DC-N1): an image record is a thumbnail with its byte count beside it.
describe('IdentityRecords image records (J13)', () => {
  it('draws a WebP avatar as a thumbnail with its byte count', () => {
    const webp = '0x52494646' + '20000000' + '57454250' + '56503820' + 'ab'.repeat(20);
    render(
      <IdentityRecords
        subject="apt.cell"
        records={[
          rec('profile.avatar', null, webp),
          rec('profile.cover', null, '0x' + 'ab'.repeat(3)),
        ]}
      />
    );
    const section = screen.getByRole('region', { name: 'What apt.cell points to' });
    const img = within(section).getByRole('img', { name: 'Avatar, a WebP image of 36 bytes' });
    expect(img).toHaveAttribute('src', expect.stringMatching(/^data:image\/webp;base64,/));
    expect(section).toHaveTextContent('WebP · 36 bytes');
    // Bytes that are no image keep the byte count.
    expect(section).toHaveTextContent('3 bytes, not text');
  });
});

describe('DotcellRoles (ID-DC4)', () => {
  it('shows owner and manager cards and every owner so far, newest last', () => {
    render(<DotcellRoles protocol={dotcellHistory()} ownerNameCount={1} />);
    const section = screen.getByRole('region', { name: 'Owner and manager' });
    // Phase 5 (A6): the roles, run on the two it names; the storage note stays beside.
    const heading = within(section).getByRole('heading', { name: 'Roles owner · manager' });
    expectPrompt(heading, 'Roles', 'owner · manager');
    expect(promptNote(heading)).toHaveTextContent(
      "stored as 20-byte lock-hash prefixes in the name's cell"
    );
    const owner = within(section).getByLabelText('Owner · holds the name');
    expect(owner).toHaveTextContent('since 2026-09-23');
    // An unresolved owner prefix stays a prefix; it is never shown as an address.
    expect(owner).toHaveTextContent('0x' + '69'.repeat(20));
    expect(within(owner).queryByRole('link')).not.toBeInTheDocument();
    const manager = within(section).getByLabelText('Manager · edits its records');
    expect(within(manager).getByRole('link')).toHaveAttribute(
      'href',
      '/mainnet/address/ckb1manager'
    );
    expect(manager).toHaveTextContent('JoyID');
    const steps = within(section).getAllByRole('listitem');
    expect(steps.map((s) => s.getAttribute('data-owner-step'))).toEqual(['registered', '', 'now']);
    expect(steps[0]).toHaveTextContent('2026-09-20');
    expect(steps[0]).toHaveTextContent('secp256k1');
    // Lane L: an owner with an address links to it (full address in the title); an unresolved
    // prefix stays text.
    const registrarLink = within(steps[0]).getByRole('link', { name: 'ckb1registrar' });
    expect(registrarLink).toHaveAttribute('href', '/mainnet/address/ckb1registrar');
    expect(registrarLink).toHaveAttribute('title', 'ckb1registrar');
    expect(within(steps[1]).getByRole('link', { name: 'ckb1joyid' })).toHaveAttribute(
      'href',
      '/mainnet/address/ckb1joyid'
    );
    expect(within(steps[2]).queryByRole('link')).toBeNull();
  });
});

// Round 3 (contracts R-A · R-5, ID-DC4): the card sub-lines the board has.
describe('DotcellRoles sub-lines (R-5)', () => {
  const managerParty = {
    hashPrefix: '0x' + '1e'.repeat(20),
    lockHash: '0x' + '1e'.repeat(32),
    address: 'ckb1manager',
    scriptName: 'JoyID',
  };
  const withEdits = (last: unknown) => ({
    ...dotcellHistory(),
    recordEdits: { count: 3, last } as Extract<ItemProtocol, { kind: 'dotcell' }>['recordEdits'],
  });
  const edit = (editor: typeof managerParty, editorRun: number) => ({
    editor,
    editorRun,
    txHash: '0x' + 'ed'.repeat(32),
    blockNumber: 18_100_000,
    timestamp: String(Date.parse('2026-09-24T00:00:00Z')),
  });
  it('says how many names the owner holds and that the manager made the last edits', () => {
    render(<DotcellRoles protocol={withEdits(edit(managerParty, 2))} ownerNameCount={1} />);
    const section = screen.getByRole('region', { name: 'Owner and manager' });
    expect(within(section).getByLabelText('Owner · holds the name')).toHaveTextContent(
      'since 2026-09-23 · this address holds 1 cell'
    );
    expect(within(section).getByLabelText('Manager · edits its records')).toHaveTextContent(
      /since \d{4}-\d{2}-\d{2} · made the last 2 record edits$/
    );
  });
  it('names the earlier manager who made the last edit, and a name never edited', () => {
    const earlier = { ...managerParty, hashPrefix: '0x' + '57'.repeat(20), address: 'ckb1old' };
    const { unmount } = render(
      <DotcellRoles protocol={withEdits(edit(earlier, 1))} ownerNameCount={4} />
    );
    expect(screen.getByLabelText('Owner · holds the name')).toHaveTextContent(
      'this address holds 4 cells'
    );
    expect(screen.getByLabelText('Manager · edits its records')).toHaveTextContent(
      'the last record edit was made by an earlier manager, ckb1old'
    );
    unmount();
    render(
      <DotcellRoles
        protocol={{ ...dotcellHistory(), recordEdits: { count: 0, last: null } }}
        ownerNameCount={1}
      />
    );
    expect(screen.getByLabelText('Manager · edits its records')).toHaveTextContent(
      'no record edit yet'
    );
  });
});

describe('DidDocumentReadout (ID-D2 / ID-D4)', () => {
  function jler(): Extract<ItemProtocol, { kind: 'did_ckb' }> {
    const p = structuredClone(didProtocolProducer) as Extract<ItemProtocol, { kind: 'did_ckb' }>;
    const v0 = p.versions[0];
    const doc = (handle: string) => ({
      alsoKnownAs: [`at://${handle}`],
      services: {
        atproto_pds: { endpoint: 'https://web5.bbs.fans', type: 'AtprotoPersonalDataServer' },
      },
      verificationMethods: { atproto: 'did:key:zQ3sh' },
    });
    const v1 = structuredClone(v0);
    v0.decoded.document = doc('JLer.web5.bbs.fans');
    v0.cell.timestamp = String(Date.parse('2025-11-27T13:30:00Z'));
    v1.decoded.document = doc('jler.web5.bbs.fans');
    v1.cell.timestamp = String(Date.parse('2025-12-10T03:23:00Z'));
    v1.cell.txHash = '0x' + '12'.repeat(32);
    p.versions = [v0, v1];
    return p;
  }

  it('reads the handle with its history, the server and the signing key', () => {
    render(<DidDocumentReadout protocol={jler()} />);
    const section = screen.getByRole('region', { name: 'DID document' });
    // Phase 5 (A3): the version is the command's args; where it is read from is the note.
    const heading = within(section).getByRole('heading', { name: 'DID document version 2 of 2' });
    expectPrompt(heading, 'DID document', 'version 2 of 2');
    expect(promptNote(heading)).toHaveTextContent(/^read from the cell$/);
    expect(section).toHaveTextContent('at://jler.web5.bbs.fans');
    expect(section).toHaveTextContent('was at://JLer.web5.bbs.fans until 2025-12-10');
    expect(within(section).getByRole('link', { name: 'https://web5.bbs.fans ↗' })).toHaveAttribute(
      'href',
      'https://web5.bbs.fans'
    );
    expect(section).toHaveTextContent('did:key:zQ3sh');
    expect(section).toHaveTextContent('unchanged since creation');
    // The API's fixed document format number is not the cell version; it stays out of tier 1.
    expect(section).not.toHaveTextContent('Document version 1');
    expect(section.querySelector('pre')).toBeNull();
  });

  it('names a field it cannot read rather than dumping it', () => {
    const p = jler();
    p.versions[1].decoded.document = { alsoKnownAs: 42 };
    render(<DidDocumentReadout protocol={p} />);
    expect(screen.getByRole('region', { name: 'DID document' })).toHaveTextContent(
      'Not read here: alsoKnownAs'
    );
  });
});

describe('DidDocumentReadout, a document it cannot read', () => {
  it('names the whole document unread rather than showing an empty identity', () => {
    const p = structuredClone(didProtocolProducer) as Extract<ItemProtocol, { kind: 'did_ckb' }>;
    p.versions[0].decoded.document = [1, { $integer: '18446744073709551615' }];
    render(<DidDocumentReadout protocol={p} />);
    expect(screen.getByRole('region', { name: 'DID document' })).toHaveTextContent(
      'Not read here: document'
    );
  });
});

describe('AmongDidCkb (ID-D3)', () => {
  it('places this identity among every did:ckb by personal data server', () => {
    render(
      <AmongDidCkb
        distribution={{
          source: 'currentCanonicalDocuments',
          totalLiveIdentities: 114,
          withoutPds: 1,
          groups: [
            { endpoint: 'https://web5.bbs.fans', count: 99 },
            { endpoint: 'https://ccfdao.example', count: 13 },
            { endpoint: 'https://example.com', count: 1 },
          ],
        }}
        endpoints={['https://web5.bbs.fans']}
        collectionHref="/identities/did:ckb"
      />
    );
    const section = screen.getByRole('region', { name: 'Among did:ckb' });
    expect(section).toHaveTextContent('114 did:ckb identities live on CKB.');
    expect(within(section).getByRole('link', { name: 'All 114 →' })).toHaveAttribute(
      'href',
      '/mainnet/identities/did:ckb'
    );
    // Phase 5 (A4): the link to the collection is the heading's note, outside its name.
    const heading = within(section).getByRole('heading', { name: 'Among did:ckb' });
    expectPrompt(heading, 'Among did:ckb');
    expect(promptNote(heading)).toContainElement(
      within(section).getByRole('link', { name: 'All 114 →' })
    );
    const mine = within(section).getByText('web5.bbs.fans').closest('li')!;
    expect(mine).toHaveTextContent('99');
    expect(mine).toHaveTextContent('this one');
    expect(within(section).getByText('none named').closest('li')).toHaveTextContent('1');
    expect(within(section).getByRole('img')).toBeInTheDocument();
  });
});
