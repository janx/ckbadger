'use client';
import Link from '@/components/ui/link';
import type { Cell } from '@/lib/api';

function fileExtent(offset: string | null, size: string | null): string {
  return offset === null || size === null
    ? 'Memory only'
    : `${offset}–${(BigInt(offset) + BigInt(size)).toString()}${size === '0' ? ' (empty)' : ''}`;
}

function hex64(value: string): string {
  return `0x${BigInt(value).toString(16)}`;
}

export function CodeCellBoard({ cell }: { cell: Cell }) {
  if (!cell.elf || !cell.codeCell) return null;
  const { header, programHeaders, sections, unassignedFileRanges, comment, riscvAttributes } =
    cell.elf;
  return (
    <section aria-label="Code cell ELF" className="border-base-border mt-6 space-y-5 border-t pt-5">
      <div>
        <h2 className="text-text-bright text-[15px]">This cell is code · RISC-V ELF</h2>
        <p className="text-text-dim mt-1 break-all font-mono text-xs">
          Version {cell.codeCell.versionHash}
        </p>
      </div>
      <dl className="grid gap-3 text-xs sm:grid-cols-3">
        <div>
          <dt className="text-text-dim">Format</dt>
          <dd>
            {header.class} · {header.endian} endian · machine {header.machine}
          </dd>
        </div>
        <div>
          <dt className="text-text-dim">Entry address</dt>
          <dd className="font-mono">{hex64(header.entry)}</dd>
        </div>
        <div>
          <dt className="text-text-dim">ELF flags</dt>
          <dd className="font-mono">0x{header.flags.toString(16)}</dd>
        </div>
        <div>
          <dt className="text-text-dim">Program table</dt>
          <dd>
            {header.programHeaderOffset} · {header.programHeaderCount} ×{' '}
            {header.programHeaderEntrySize} bytes
          </dd>
        </div>
        <div>
          <dt className="text-text-dim">Section table</dt>
          <dd>
            {header.sectionHeaderOffset} · {header.sectionHeaderCount} ×{' '}
            {header.sectionHeaderEntrySize} bytes
          </dd>
        </div>
        <div>
          <dt className="text-text-dim">Header size</dt>
          <dd>{header.headerSize} bytes</dd>
        </div>
      </dl>
      <div>
        <h3 className="text-text-bright mb-2 text-sm">Program headers</h3>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[620px] text-left text-xs">
            <thead className="text-text-dim">
              <tr>
                <th>Index</th>
                <th>Type</th>
                <th>Flags</th>
                <th>File bytes</th>
                <th>Virtual address</th>
                <th>Memory size</th>
                <th>Align</th>
              </tr>
            </thead>
            <tbody>
              {programHeaders.map((entry) => (
                <tr key={entry.index} className="border-base-border border-t">
                  <td>{entry.index}</td>
                  <td>{entry.kind}</td>
                  <td>0x{entry.flags.toString(16)}</td>
                  <td>{fileExtent(entry.fileOffset, entry.fileSize)}</td>
                  <td>{hex64(entry.virtualAddress)}</td>
                  <td>{entry.memorySize}</td>
                  <td>{entry.alignment}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
      <div>
        <h3 className="text-text-bright mb-2 text-sm">Sections</h3>
        <div className="overflow-x-auto">
          <table className="w-full min-w-[620px] text-left text-xs">
            <thead className="text-text-dim">
              <tr>
                <th>Name</th>
                <th>Type</th>
                <th>Flags</th>
                <th>File bytes</th>
                <th>Address</th>
                <th>Memory size</th>
                <th>Align</th>
              </tr>
            </thead>
            <tbody>
              {sections.map((entry) => (
                <tr key={entry.index} className="border-base-border border-t">
                  <td>{entry.name || '(unnamed)'}</td>
                  <td>{entry.kind}</td>
                  <td>{hex64(entry.flags)}</td>
                  <td>{fileExtent(entry.fileOffset, entry.fileSize)}</td>
                  <td>{hex64(entry.address)}</td>
                  <td>{entry.memorySize}</td>
                  <td>{entry.alignment}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
        <p className="text-text-dim mt-2 text-xs">
          NOBITS sections occupy memory after loading, not file bytes. Program and section ranges
          may overlap in the same file.
        </p>
      </div>
      <div className="grid gap-3 text-xs sm:grid-cols-2">
        <div>
          <h3 className="text-text-bright text-sm">Unassigned file bytes</h3>
          {unassignedFileRanges.length ? (
            unassignedFileRanges.map((gap) => (
              <p key={gap.start}>
                {gap.start}–{gap.end - 1} · {gap.end - gap.start} bytes · purpose unspecified
              </p>
            ))
          ) : (
            <p>None</p>
          )}
        </div>
        <div>
          <h3 className="text-text-bright text-sm">Build evidence</h3>
          <p>.comment: {comment ?? 'Absent'}</p>
          <p>.riscv.attributes: {riscvAttributes ?? 'Absent'}</p>
        </div>
      </div>
      <div>
        <h3 className="text-text-bright text-sm">Current runtime references</h3>
        {cell.codeCell.currentReferences.length ? (
          <ul className="mt-2 space-y-2 text-xs">
            {cell.codeCell.currentReferences.map((reference) => (
              <li key={`${reference.referenceHash}-${reference.hashType}`} className="break-all">
                <Link
                  href={`/script/${reference.referenceHash}?hashType=${reference.hashType}`}
                  className="text-[#7aa7d6]"
                >
                  {reference.hashType} · {reference.referenceHash}
                </Link>
                <span className="text-text-dim">
                  {' '}
                  · {reference.lockLiveCellsCount} live locks · {reference.typeLiveCellsCount} live
                  types
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-text-dim mt-2 text-xs">
            No indexed current consumers for these reference forms.
          </p>
        )}
        <p className="text-text-dim mt-2 text-xs">
          Type references are resolved against current live code cells. A historical transaction’s
          executable is established by its actual cell deps.
        </p>
      </div>
      <div className="border-base-border border-t pt-4">
        <h3 className="text-text-bright text-sm">Observed Type ID versions</h3>
        {cell.codeCell.typeIdVersions.length ? (
          cell.codeCell.typeIdVersions.map((item) => (
            <p
              key={`${item.typeScriptHash}:${item.codeCellTxHash}:${item.codeCellOutputIndex}`}
              className="mt-2 break-all text-xs"
            >
              <span className={item.thisBinary ? 'text-text-bright' : 'text-text-dim'}>
                Version {item.ordinal}
                {item.thisBinary ? ' · these bytes' : ''}
                {item.reactivation ? ' · reactivated' : ''}
              </span>{' '}
              <Link
                href={`/cell/${item.codeCellTxHash}-${item.codeCellOutputIndex}`}
                className="text-[#7aa7d6]"
              >
                {item.codeCellTxHash}:{item.codeCellOutputIndex}
              </Link>{' '}
              · observed block #{item.observedAtBlock} · {item.typeScriptHash}
            </p>
          ))
        ) : (
          <p className="text-text-dim mt-2 text-xs">
            No registered Type ID version history for this binary.
          </p>
        )}
      </div>
      <div className="border-base-border border-t pt-4">
        <h3 className="text-text-bright text-sm">Same-transaction upgrade cohort</h3>
        <p className="text-text-dim mt-1 text-xs">
          Each pair continues the same exact type script from a consumed input to an ELF output in
          this transaction.
        </p>
        {cell.codeCell.upgradeCohort.length ? (
          cell.codeCell.upgradeCohort.map((item) => (
            <p
              key={`${item.currentTxHash}:${item.currentOutputIndex}`}
              className="mt-2 break-all text-xs"
            >
              <Link
                href={`/cell/${item.previousTxHash}-${item.previousOutputIndex}`}
                className="text-[#7aa7d6]"
              >
                Input {item.previousTxHash}:{item.previousOutputIndex}
              </Link>{' '}
              →{' '}
              <Link
                href={`/cell/${item.currentTxHash}-${item.currentOutputIndex}`}
                className="text-[#7aa7d6]"
              >
                output #{item.currentOutputIndex}
              </Link>{' '}
              ·{' '}
              {item.binaryChanged === null
                ? 'previous binary hash unavailable'
                : item.binaryChanged
                  ? 'binary changed'
                  : 'same binary'}{' '}
              · {item.typeScriptHash}
            </p>
          ))
        ) : (
          <p className="text-text-dim mt-2 text-xs">
            No same-transaction ELF continuation observed.
          </p>
        )}
      </div>
    </section>
  );
}
