'use client';
import Link from '@/components/ui/link';
import type { Cell } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { CELL_COLORS } from '@/lib/design-tokens';

const DATA_HASH_TYPES = new Set(['data', 'data1', 'data2']);

function liveUsers(reference: { lockLiveCellsCount: number; typeLiveCellsCount: number }): number {
  return reference.lockLiveCellsCount + reference.typeLiveCellsCount;
}

/**
 * "This cell is code" (CellCode board): how scripts name this binary — by the Type ID's type
 * hash (follows upgrades) or by its data hash (pins it) — with the live cells that use each way,
 * and what built it, read from the binary's own .comment and .riscv.attributes sections. The
 * ELF's parts are not tabled here: the microscope below names every byte.
 */
export function CodeCellBoard({ cell }: { cell: Cell }) {
  if (!cell.elf || !cell.codeCell) return null;
  const { codeCell, elf } = cell;
  const names = (cell.codeCellOf ?? []).map((script) => script.name);
  const typeHash = cell.cellOutputMolecule?.typeScriptHash ?? null;
  const byType = typeHash
    ? codeCell.currentReferences.filter(
        (reference) => reference.hashType === 'type' && reference.referenceHash === typeHash
      )
    : [];
  const byData = codeCell.currentReferences.filter(
    (reference) =>
      DATA_HASH_TYPES.has(reference.hashType) && reference.referenceHash === codeCell.versionHash
  );
  const otherReferences = codeCell.currentReferences.filter(
    (reference) => !byType.includes(reference) && !byData.includes(reference)
  );
  const typeUsers = byType.reduce((n, reference) => n + liveUsers(reference), 0);
  const dataUsers = byData.reduce((n, reference) => n + liveUsers(reference), 0);
  const who = names.length ? `${names.join(' / ')} ` : '';
  const comment = elf.comment?.split(' · ').filter(Boolean) ?? [];
  const stripped = !elf.sections.some((section) => section.name === '.symtab');
  return (
    <section aria-labelledby="code-cell-heading" className="min-w-0 space-y-3">
      <h2 id="code-cell-heading" className="text-text-bright text-[15px] font-medium">
        This cell is code
      </h2>
      <p className="max-w-[860px] text-[13px] leading-relaxed">
        Its data is a RISC-V program. A script names it in one of two ways, and each way decides
        what happens when the code changes.
      </p>
      <div className="grid gap-7 text-xs leading-relaxed md:grid-cols-3">
        <div
          data-testid="code-by-type-hash"
          className="flex min-w-0 flex-col gap-1 border-t-2 pt-2.5"
          style={{ borderTopColor: CELL_COLORS.type }}
        >
          <span className="text-text-dim">by type hash · hash_type type</span>
          {typeHash ? (
            <>
              <span className="text-text-bright [overflow-wrap:anywhere]">{typeHash}</span>
              <span>
                <span className="text-text-bright">{typeUsers.toLocaleString('en-US')}</span> live{' '}
                {who}cells name it this way. They follow upgrades: whatever cell carries this type
                id next is what they run.
              </span>
            </>
          ) : (
            <span className="text-text-dim">
              No type script: no one can name this binary by type hash.
            </span>
          )}
        </div>
        <div
          data-testid="code-by-data-hash"
          className="flex min-w-0 flex-col gap-1 border-t-2 pt-2.5"
          style={{ borderTopColor: CELL_COLORS.data }}
        >
          <span className="text-text-dim">by data hash · hash_type data1 / data2</span>
          <span className="text-text-bright [overflow-wrap:anywhere]">{codeCell.versionHash}</span>
          <span>
            <span className="text-text-bright">{dataUsers.toLocaleString('en-US')}</span> live cells
            pin this exact binary. A pin survives any upgrade, and keeps working if anyone redeploys
            the same bytes.
          </span>
        </div>
        <div
          data-testid="code-built-with"
          className="flex min-w-0 flex-col gap-1 border-t-2 border-[#343c50] pt-2.5"
        >
          <span className="text-text-dim">built with</span>
          {comment.length ? (
            comment.map((piece) => (
              <span key={piece} className="text-text-bright [overflow-wrap:anywhere]">
                {piece}
              </span>
            ))
          ) : (
            <span className="text-text-dim">No .comment section</span>
          )}
          <span>
            {elf.riscvAttributes ? `target ${elf.riscvAttributes} · ` : ''}entry 0x
            {BigInt(elf.header.entry).toString(16)}
            {stripped ? ' · no symbol table' : ''}. Read from the binary&apos;s own .comment and
            .riscv.attributes sections.
          </span>
        </div>
      </div>
      {otherReferences.length > 0 && (
        <ul className="text-text-dim space-y-1 text-[11px]">
          {otherReferences.map((reference) => (
            <li key={`${reference.referenceHash}-${reference.hashType}`} className="break-all">
              Also named as{' '}
              <Link
                href={`/script/${reference.referenceHash}?hashType=${reference.hashType}`}
                className="text-[#7aa7d6]"
              >
                {reference.hashType} · {reference.referenceHash}
              </Link>{' '}
              · {liveUsers(reference).toLocaleString('en-US')} live cells
            </li>
          ))}
        </ul>
      )}
      <details className="text-xs">
        <summary className="text-text-dim hover:text-text cursor-pointer">
          Type ID lineage of these bytes
        </summary>
        <div className="mt-2 space-y-1.5">
          {codeCell.typeIdVersions.length ? (
            codeCell.typeIdVersions.map((item) => (
              <p
                key={`${item.typeScriptHash}:${item.codeCellTxHash}:${item.codeCellOutputIndex}`}
                className="break-all"
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
                · observed block {formatBlockNumber(item.observedAtBlock)}
              </p>
            ))
          ) : (
            <p className="text-text-dim">No registered Type ID version history for this binary.</p>
          )}
          {codeCell.upgradeCohort.map((item) => (
            <p key={`${item.currentTxHash}:${item.currentOutputIndex}`} className="break-all">
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
                  : 'same binary'}
            </p>
          ))}
        </div>
      </details>
    </section>
  );
}
