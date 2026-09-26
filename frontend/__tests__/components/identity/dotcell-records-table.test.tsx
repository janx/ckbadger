import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { render } from '@/__tests__/utils/test-utils';
import { DotCellRecordsTable } from '@/components/identity/dotcell-records-table';
import type { DotCellRecord } from '@/lib/api';

/** testnet `maria.cell`'s `address.309` record, resolved to a lock. */
const ADDRESS =
  'ckt1qrfrwcdnvssswdwpn3s9v8fp87emat306ctjwsm3nmlkjg8qyza2cqgqq9x75zu4l7gld606r6eyd00m4lzy3zkxkq4nywzu';

const RECORDS: DotCellRecord[] = [
  {
    key: 'address.309',
    label: '',
    valueHex: '0x636b7431',
    valueUtf8: ADDRESS,
    ttl: 300,
    decodedAddress: {
      address: ADDRESS,
      lockHash: '0x57d926a44d83fc13b21ce037b1e31f4223e3c867cfa3f60e1324d5bfd5cd742d',
    },
  },
  {
    key: 'avatar.raw',
    label: 'bin',
    valueHex: '0xfffe00',
    valueUtf8: null,
    ttl: 600,
    decodedAddress: null,
  },
];

describe('DotCellRecordsTable', () => {
  it('links a decoded CKB address and shows non-UTF-8 values as hex', () => {
    render(<DotCellRecordsTable records={RECORDS} />);

    const rows = screen.getAllByRole('row');
    // Header row + one row per record.
    expect(rows).toHaveLength(3);
    expect(within(rows[1]).getByText('address.309')).toBeInTheDocument();
    expect(within(rows[1]).getByRole('link', { name: ADDRESS })).toHaveAttribute(
      'href',
      `/mainnet/address/${ADDRESS}`
    );
    expect(within(rows[1]).getByText('300')).toBeInTheDocument();

    expect(within(rows[2]).getByText('avatar.raw')).toBeInTheDocument();
    expect(within(rows[2]).getByText('bin')).toBeInTheDocument();
    // Nothing is guessed for bytes that are not UTF-8: the hex is shown.
    expect(within(rows[2]).getByText('0xfffe00')).toBeInTheDocument();
    expect(within(rows[2]).queryByRole('link')).not.toBeInTheDocument();
    expect(within(rows[2]).getByText('600')).toBeInTheDocument();
  });

  it('says so when a name has no records', () => {
    render(<DotCellRecordsTable records={[]} />);

    expect(screen.getByText('No records set.')).toBeInTheDocument();
    expect(screen.queryByRole('table')).not.toBeInTheDocument();
  });
});
