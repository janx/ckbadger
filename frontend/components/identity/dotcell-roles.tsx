import type { ItemProtocol, DotCellPartyRef } from '@/lib/api';
import Link from '@/components/ui/link';
import { PartyLine } from '@/components/identity/identity-overview';
import { PromptHeading } from '@/components/ui/prompt-heading';
import { ownerRun, ownerRuns } from '@/lib/identity-term';
import { shortenAddress, shortenHash } from '@/lib/shorten';
import { cn } from '@/lib/utils';
import { countNoun } from '@/lib/format-count';
import { Utc8Date } from '@/components/ui/utc8-date';

type DotcellProtocol = Extract<ItemProtocol, { kind: 'dotcell' }>;

/**
 * A named party in a line or an owner's step: its address (else its lock hash) at the row size
 * (U-09, PB-19: the role cards, the owners and the metrics of a name write it at one length), or
 * the 20-byte prefix whole when no lock was resolved.
 */
function partyShort(party: DotCellPartyRef): string {
  if (!party.lockHash) return party.hashPrefix;
  return party.address ? shortenAddress(party.address, 'row') : shortenHash(party.lockHash, 'row');
}

/**
 * "Owner and manager" (ID-DC4): the two roles a `.cell` name stores as 20-byte lock-hash
 * prefixes in its data, each from when it took over, and every owner so far. A prefix the API
 * could not resolve to exactly one lock stays a prefix, never an address.
 */
/** Round 3 (R-5): the manager card's sub-line from `/protocol` `recordEdits` (ID-DC4). */
function recordEditsLine(protocol: DotcellProtocol, manager: DotCellPartyRef): string {
  const last = protocol.recordEdits.last;
  if (last === null) {
    if (protocol.recordEdits.count !== 0)
      throw new Error(`${protocol.itemId}: ${protocol.recordEdits.count} record edits but no last`);
    return 'no record edit yet';
  }
  if (!Number.isSafeInteger(last.editorRun) || last.editorRun < 1)
    throw new Error(`${protocol.itemId}: invalid record editor run ${last.editorRun}`);
  if (last.editor.hashPrefix === manager.hashPrefix)
    return last.editorRun === 1
      ? 'made the last record edit'
      : `made the last ${last.editorRun.toLocaleString('en-US')} record edits`;
  return `the last record edit was made by an earlier manager, ${partyShort(last.editor)}`;
}

/**
 * The cards also carry the round-3 facts (contracts R-A · R-5): how many live names the owner
 * prefix holds (`ownerNameCount`, from the name's detail) and who made the last record edits.
 */
export function DotcellRoles({
  protocol,
  ownerNameCount,
}: {
  protocol: DotcellProtocol;
  ownerNameCount: number;
}) {
  if (!Number.isSafeInteger(ownerNameCount) || ownerNameCount < 0)
    throw new Error(`${protocol.itemId}: invalid owner name count ${ownerNameCount}`);
  const versions = protocol.versions;
  const current = versions.at(-1);
  if (!current) throw new Error(`${protocol.itemId}: .cell name has no versions`);
  const owner = ownerRun(versions, (v) => v.decoded.owner.hashPrefix);
  const manager = ownerRun(versions, (v) => v.decoded.manager.hashPrefix);
  const runs = ownerRuns(versions, (v) => v.decoded.owner.hashPrefix);
  const cards = [
    {
      label: 'Owner · holds the name',
      party: current.decoded.owner,
      since: owner.since.cell.timestamp,
      note: `this address holds ${ownerNameCount.toLocaleString('en-US')} ${countNoun(ownerNameCount, 'cell', 'cells')}`,
      accent: true,
    },
    {
      label: 'Manager · edits its records',
      party: current.decoded.manager,
      since: manager.since.cell.timestamp,
      note: recordEditsLine(protocol, current.decoded.manager),
      accent: false,
    },
  ];
  return (
    <section aria-label="Owner and manager" className="min-w-0 space-y-3">
      <PromptHeading
        command="Roles"
        args="owner · manager"
        note="stored as 20-byte lock-hash prefixes in the name's cell"
      />
      <div className="grid min-w-0 gap-4 sm:grid-cols-2">
        {cards.map((c) => (
          <div
            key={c.label}
            role="group"
            aria-label={c.label}
            className={cn(
              'flex min-w-0 flex-col gap-1 border px-4 py-3.5 [overflow-wrap:anywhere]',
              c.accent ? 'border-identity/50' : 'border-base-border'
            )}
          >
            <span className="text-text-dim text-small">{c.label}</span>
            <span>
              <PartyLine party={c.party} size="row" />
            </span>
            <span className="text-text-dim text-small">
              since <Utc8Date timestamp={c.since} /> · {c.note}
            </span>
          </div>
        ))}
      </div>
      <h3 className="text-text-dim text-small pt-1 font-normal">Owners so far</h3>
      <ol className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-stretch">
        {runs.map((run, i) => {
          const step = i === runs.length - 1 ? 'now' : i === 0 ? 'registered' : '';
          const party = run.since.decoded.owner;
          return (
            <li
              key={`${run.owner}:${run.since.cell.txHash}`}
              data-owner-step={step}
              className={cn(
                'flex min-w-0 flex-1 flex-col gap-0.5 border px-3 py-2.5',
                step === 'now' ? 'border-identity/60' : 'border-base-border border-dashed'
              )}
            >
              <span className="text-text-dim text-meta">
                <Utc8Date timestamp={run.since.cell.timestamp} />
                {step ? ` · ${step}` : ''}
              </span>
              <span
                className={cn(
                  'text-body truncate',
                  step === 'now' ? 'text-text-bright' : 'text-text'
                )}
              >
                {party.address ? (
                  <Link
                    href={`/address/${party.address}`}
                    title={party.address}
                    className="hover:text-jade"
                  >
                    {partyShort(party)}
                  </Link>
                ) : (
                  partyShort(party)
                )}
              </span>
              <span className="text-text-dim text-meta">
                {party.scriptName ?? 'lock not named'}
              </span>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
