import { render, screen, within } from '@/__tests__/utils/test-utils';
import { IdentityRecords } from '@/components/identity/identity-records';
import { DotcellRoles } from '@/components/identity/dotcell-roles';
import { DidDocumentReadout, AmongDidCkb } from '@/components/identity/did-readout';
import type { ItemProtocol } from '@/lib/api';
import {
  didProtocolProducer,
  dotcellProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';

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
  });

  it('says plainly when no records are set', () => {
    render(<IdentityRecords subject="valar.bit" records={[]} />);
    expect(screen.getByRole('region', { name: 'What valar.bit points to' })).toHaveTextContent(
      'No records are set.'
    );
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

describe('DotcellRoles (ID-DC4)', () => {
  it('shows owner and manager cards and every owner so far, newest last', () => {
    render(<DotcellRoles protocol={dotcellHistory()} />);
    const section = screen.getByRole('region', { name: 'Owner and manager' });
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
    expect(section).toHaveTextContent('read from the cell · version 2 of 2');
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
    const mine = within(section).getByText('web5.bbs.fans').closest('li')!;
    expect(mine).toHaveTextContent('99');
    expect(mine).toHaveTextContent('this one');
    expect(within(section).getByText('none named').closest('li')).toHaveTextContent('1');
    expect(within(section).getByRole('img')).toBeInTheDocument();
  });
});
