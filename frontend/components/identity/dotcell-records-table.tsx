'use client';

import Link from '@/components/ui/link';
import type { DotCellRecord } from '@/lib/api';

interface Props {
  records: DotCellRecord[];
}

/**
 * A `.cell` name's records as the API decodes them. Shared by the identity
 * page (the current records) and the transaction page (the records payload a
 * creating witness carries), so both read one table.
 */
export function DotCellRecordsTable({ records }: Props) {
  if (records.length === 0) {
    return <div className="text-text-dim p-4 font-mono text-sm">No records set.</div>;
  }
  return (
    <div className="overflow-x-auto">
      <table className="w-full text-left font-mono text-sm">
        <thead>
          <tr className="border-base-border text-text-dim border-b text-xs uppercase tracking-wider">
            <th className="px-4 py-2 font-normal">Key</th>
            <th className="px-4 py-2 font-normal">Label</th>
            <th className="px-4 py-2 font-normal">Value</th>
            <th className="px-4 py-2 text-right font-normal">TTL</th>
          </tr>
        </thead>
        <tbody>
          {records.map((record, index) => (
            <tr
              key={`${record.key}-${record.label}-${index}`}
              className="border-base-border border-b last:border-b-0"
            >
              <td className="text-text-bright px-4 py-2 align-top">{record.key}</td>
              <td className="text-text-dim px-4 py-2 align-top">{record.label}</td>
              <td className="text-text break-all px-4 py-2 align-top">
                {record.decodedAddress ? (
                  <Link
                    href={`/address/${record.decodedAddress.address}`}
                    className="text-aqua hover:underline"
                  >
                    {record.decodedAddress.address}
                  </Link>
                ) : (
                  // A record whose bytes are not UTF-8 shows its
                  // hex; nothing is guessed on its behalf.
                  (record.valueUtf8 ?? record.valueHex)
                )}
              </td>
              <td className="text-text-dim px-4 py-2 text-right align-top">{record.ttl}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
