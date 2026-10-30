import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ItemEvidence } from '@/components/asset/item-evidence';
import { DetailTier2 } from '@/components/ui/detail-layout';
import {
  BitCellAccountNow,
  DidStored,
  DotbitAccountData,
  DotcellStored,
  identityDataTitle,
} from '@/components/identity/identity-evidence';
import { api, type ItemProtocol } from '@/lib/api';
import {
  bitCellProtocolProducer,
  didProtocolProducer,
  dotbitV4ProtocolProducer,
  dotcellProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { dotcellItem } from '@/__tests__/utils/item-read-fixtures';

afterEach(() => vi.restoreAllMocks());

describe('identity tier 2 (ID-C9 / ID-B1 / ID-B2 / ID-BC2 / ID-D4 / ID-DC6 / ID-DC7)', () => {
  it('titles the data map by what the cell is and its size, not the protocol slug', () => {
    render(
      <DetailTier2>
        <ItemEvidence
          protocol={dotbitV4ProtocolProducer as ItemProtocol}
          title={identityDataTitle('dotbit')}
        />
      </DetailTier2>
    );
    const heading = screen.getByRole('heading', { name: 'Account cell data · 80 bytes' });
    expect(heading).toHaveClass('text-[13px]');
    expect(screen.queryByText(/dotbit cell data/)).not.toBeInTheDocument();
    expect(identityDataTitle('dotcell')(dotcellProtocolProducer.versions[0] as never)).toBe(
      'Name cell data · layout v3 · 103 bytes'
    );
  });

  it('keeps .bit account internals in tier 2 with owner and manager keys as stored', () => {
    render(
      <DetailTier2>
        <DotbitAccountData protocol={dotbitV4ProtocolProducer as never} />
      </DetailTier2>
    );
    const section = screen.getByRole('region', { name: 'Account data, from the witness' });
    expect(screen.getByRole('heading', { name: 'Account data, from the witness' })).toHaveClass(
      'text-[13px]'
    );
    expect(section).toHaveTextContent('status0x00 · raw byte, as stored');
    expect(section).toHaveTextContent('witness version4');
    expect(section).toHaveTextContent(
      'owneralgorithm 3 · 0x6666666666666666666666666666666666666666'
    );
    expect(section).toHaveTextContent('records1 entry');
  });

  it('lists the witness timing and sub-account fields, "never" for a zero time (ID-B1)', () => {
    const p = structuredClone(dotbitV4ProtocolProducer) as Extract<
      ItemProtocol,
      { kind: 'dotbit' }
    >;
    Object.assign(p.versions[0].decoded, {
      updatedAt: null,
      lastTransferAccountAt: '1721010954',
      lastEditManagerAt: '0',
      lastEditRecordsAt: '1717492502',
      enableSubAccount: 1,
      renewSubAccountPrice: '100000000',
    });
    render(<DotbitAccountData protocol={p} />);
    const section = screen.getByRole('region', { name: 'Account data, from the witness' });
    expect(section).toHaveTextContent('last_transfer_account_at2024-07-15');
    expect(section).toHaveTextContent('last_edit_manager_atnever');
    expect(section).toHaveTextContent('last_edit_records_at2024-06-04');
    expect(section).toHaveTextContent('enable_sub_account1');
    expect(section).toHaveTextContent('renew_sub_account_price100,000,000 · raw');
    expect(section).not.toHaveTextContent('updated_at');
  });

  it('names the .bit account a .bit Cell points to as it is now, not as at minting', () => {
    const p = structuredClone(bitCellProtocolProducer) as Extract<
      ItemProtocol,
      { kind: 'bit_cell' }
    >;
    p.versions[0].decoded.linkedAccount = {
      id: '0x' + 'c9'.repeat(20),
      name: '888899.bit',
      isLive: false,
      registeredAt: '1745499442',
      expiredAt: '1777035442',
    };
    render(<BitCellAccountNow protocol={p} />);
    const section = screen.getByRole('region', { name: 'The .bit account it names, now' });
    expect(within(section).getByRole('link', { name: '888899.bit' })).toHaveAttribute(
      'href',
      `/mainnet/identities/dotbit/0x${'c9'.repeat(20)}`
    );
    expect(section).toHaveTextContent('not live');
    expect(section).toHaveTextContent('2025-04-24');
  });

  it('keeps the did:ckb document as stored, with its layout and local ID, in tier 2', () => {
    render(<DidStored protocol={didProtocolProducer as never} />);
    const section = screen.getByRole('region', { name: 'Document as stored · CBOR, 208 bytes' });
    expect(section).toHaveTextContent('local IDlocal-1');
    expect(section).toHaveTextContent('data layoutV1 (union variant 0)');
    expect(section.querySelector('pre')).toHaveTextContent('"alsoKnownAs"');
  });

  it('keeps the exact large CBOR integer and a non-map document whole (moved from IdentityDocument)', () => {
    const { unmount } = render(<DidStored protocol={didProtocolProducer as never} />);
    expect(screen.getByRole('region', { name: /Document as stored/ })).toHaveTextContent(
      '18446744073709551615'
    );
    unmount();
    const p = structuredClone(didProtocolProducer) as Extract<ItemProtocol, { kind: 'did_ckb' }>;
    const tagged = { $tag: '42', value: { $bytes: '0x00ff' } };
    p.versions[0].decoded.document = tagged;
    render(<DidStored protocol={p} />);
    expect(
      screen.getByRole('region', { name: /Document as stored/ }).querySelector('pre')!.textContent
    ).toBe(JSON.stringify(tagged, null, 2));
  });

  it('shows .cell records as stored, the shared lock and the ring successor by name', async () => {
    const p = structuredClone(dotcellProtocolProducer) as Extract<
      ItemProtocol,
      { kind: 'dotcell' }
    >;
    p.versions[0].decoded.nextId = '0x' + '3a'.repeat(20);
    p.versions[0].decoded.records = [
      {
        key: 'profile.avatar',
        label: '',
        valueHex: '0x' + '52'.repeat(400),
        valueUtf8: null,
        ttl: 300,
        decodedAddress: null,
      },
    ];
    vi.spyOn(api, 'getDotCellItemDetail').mockResolvedValue({
      ...dotcellItem(),
      identityId: '0x' + '3a'.repeat(20),
      name: 'ckb.cell',
    });
    render(<DotcellStored protocol={p} item={dotcellItem()} />);
    const records = screen.getByRole('region', { name: 'Records as stored · witness' });
    expect(records).toHaveTextContent('profile.avatar');
    expect(records).toHaveTextContent('ttl 300');
    expect(records).not.toHaveTextContent('52'.repeat(40));
    const cell = screen.getByRole('region', { name: 'Name cell' });
    expect(cell).toHaveTextContent('the namespace, the same for every name');
    expect(cell).toHaveTextContent('ownership is the owner field in the data');
    expect(await within(cell).findByRole('link', { name: 'ckb.cell' })).toHaveAttribute(
      'href',
      `/mainnet/identities/dotcell/0x${'3a'.repeat(20)}`
    );
  });
});
