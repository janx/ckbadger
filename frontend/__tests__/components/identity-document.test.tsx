import type { ItemProtocol, BitCellProtocol, DotCellRecord } from '@/lib/api';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import {
  IdentityDocument,
  MnftPermissions,
  NamedParty,
} from '@/components/asset/identity-document';
import {
  didProtocolProducer,
  dotbitV4ProtocolProducer,
  bitCellProtocolProducer,
  bitCellLegacyProtocolProducer,
  dotcellProtocolProducer,
  mnftProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';
it('shows a friendly DID document and preserves the exact large CBOR integer', () => {
  render(<IdentityDocument protocol={didProtocolProducer} />);
  expect(screen.getByText('did:ckb:ceirceirceirceirceirceirceirceir')).toBeInTheDocument();
  expect(screen.getByRole('link', { name: 'https://pds.example ↗' })).toHaveAttribute(
    'href',
    'https://pds.example'
  );
  expect(screen.getByText(/18446744073709551615/)).toBeInTheDocument();
  expect(screen.getByText(/No expiry/)).toBeInTheDocument();
  expect(screen.getByLabelText('Identity term and document versions')).toHaveTextContent(
    'Version 1'
  );
});
it('keeps .bit protocol owner and manager separate from cell ownership and exposes records', () => {
  render(<IdentityDocument protocol={dotbitV4ProtocolProducer} />);
  const d = dotbitV4ProtocolProducer.versions[0].decoded;
  const rights = screen.getByLabelText('Account rights');
  expect(rights).toHaveTextContent(`owner · algorithm ${d.owner.algorithm}`);
  expect(rights).toHaveTextContent(`manager · algorithm ${d.manager.algorithm}`);
  expect(rights).toHaveTextContent(d.owner.argsHex);
  for (const record of d.records)
    expect(screen.getByLabelText('Account records')).toHaveTextContent(
      record.value ?? record.valueHex
    );
  expect(screen.getByText(/witness version 4/)).toBeInTheDocument();
});
it.each([bitCellProtocolProducer, bitCellLegacyProtocolProducer])(
  'does not interpret .bit Cell expiry as consumed-cell state',
  (protocol) => {
    render(<IdentityDocument protocol={protocol} />);
    expect(screen.getByText(/live cell can remain after that date/)).toBeInTheDocument();
    const d = protocol.versions[0].decoded as BitCellProtocol;
    expect(screen.getByText(d.account)).toBeInTheDocument();
    if (d.linkedAccount)
      expect(screen.getByRole('link', { name: /→/ })).toHaveAttribute(
        'href',
        `/mainnet/identities/dotbit/${d.linkedAccount.id}`
      );
  }
);
it('preserves .cell unresolved named prefixes and protocol records without fake address links', () => {
  render(<IdentityDocument protocol={dotcellProtocolProducer} />);
  const rights = screen.getByLabelText('Named ownership');
  expect(rights).toHaveTextContent('Named owner');
  expect(rights).toHaveTextContent(dotcellProtocolProducer.versions[0].decoded.owner.hashPrefix);
  expect(within(rights).queryByRole('link')).not.toBeInTheDocument();
  expect(rights).toHaveTextContent("separate from the physical cell's lock owner");
});
it('names .cell records by their protocol key and keeps binary values to one line (ID-DC1/2)', () => {
  const protocol = structuredClone(dotcellProtocolProducer);
  const webp = `0x52494646${'d0'.repeat(400)}57454250`;
  (protocol.versions.at(-1)!.decoded as { records: DotCellRecord[] }).records = [
    {
      key: 'address.0',
      label: '',
      valueHex: '0x31',
      valueUtf8: '1',
      ttl: 300,
      decodedAddress: null,
    },
    {
      key: 'profile.cover',
      label: '',
      valueHex: webp,
      valueUtf8: null,
      ttl: 300,
      decodedAddress: null,
    },
    { key: '', label: '', valueHex: '0x00', valueUtf8: null, ttl: 300, decodedAddress: null },
  ];
  render(<IdentityDocument protocol={protocol} />);
  const records = screen.getByRole('region', { name: 'Name records' });
  const keys = Array.from(records.querySelectorAll('dt')).map((dt) => dt.textContent);
  expect(keys).toEqual(['address.0', 'profile.cover', '(empty key)']);
  expect(records).toHaveTextContent('0x52494646d0d0d0d0d0…57454250');
  expect(records).not.toHaveTextContent('d0'.repeat(40));
});
it('links only a uniquely resolved named party', () => {
  render(
    <dl>
      <NamedParty
        label="Manager"
        party={{
          hashPrefix: '0x11',
          lockHash: '0x22',
          address: 'ckb1manager',
          scriptName: 'JoyID',
        }}
      />
    </dl>
  );
  expect(screen.getByRole('link')).toHaveAttribute('href', '/mainnet/address/ckb1manager');
  expect(screen.getByText('JoyID')).toBeInTheDocument();
});
it('uses official configure permissions and keeps locked state independent', () => {
  const d = mnftProtocolProducer.versions[0].decoded;
  render(
    <MnftPermissions
      permissions={d.permissions}
      configure={d.configure}
      state={{ claimed: d.claimed, locked: d.locked, value: d.state }}
    />
  );
  const panel = screen.getByLabelText('mNFT permissions');
  expect(panel).toHaveTextContent('Configure 0xc0');
  expect(panel).toHaveTextContent('Destroy before claimForbidden');
  expect(panel).toHaveTextContent('Transfer after claimAllowed');
  expect(panel).toHaveTextContent('locked state prevents transfer');
});

it.each([
  ['array', [1, { $integer: '18446744073709551615' }]],
  ['primitive', 'a valid document'],
  ['tag', { $tag: '42', value: { $bytes: '0x00ff' } }],
])('preserves valid non-map or tagged DID CBOR: %s', (_label, document) => {
  const protocol: ItemProtocol = structuredClone(didProtocolProducer);
  protocol.versions[0].decoded.document = document;
  render(<IdentityDocument protocol={protocol} />);
  expect(screen.getByLabelText('DID document')).toHaveTextContent(
    typeof document === 'string' ? document : JSON.stringify(document, null, 2).replace(/\s+/g, ' ')
  );
  expect(screen.getByRole('heading', { name: 'DID document' })).toHaveClass('text-[15px]');
});
