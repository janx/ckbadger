import type { ItemProtocol, DotCellPartyRef } from '@/lib/api';
import { NamedParty } from '@/components/asset/identity-document';
import { ownerRun, ownerRuns } from '@/lib/identity-term';
import { formatUtc8Date, truncateHash } from '@/lib/utils';
import { cn } from '@/lib/utils';

type DotcellProtocol = Extract<ItemProtocol, { kind: 'dotcell' }>;

function partyShort(party: DotCellPartyRef): string {
  return party.lockHash ? truncateHash(party.address ?? party.lockHash, 13, 8) : party.hashPrefix;
}

/**
 * "Owner and manager" (ID-DC4): the two roles a `.cell` name stores as 20-byte lock-hash
 * prefixes in its data, each from when it took over, and every owner so far. A prefix the API
 * could not resolve to exactly one lock stays a prefix, never an address.
 */
export function DotcellRoles({ protocol }: { protocol: DotcellProtocol }) {
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
      accent: true,
    },
    {
      label: 'Manager · edits its records',
      party: current.decoded.manager,
      since: manager.since.cell.timestamp,
      accent: false,
    },
  ];
  return (
    <section aria-label="Owner and manager" className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 className="text-text-bright text-[15px] font-medium">Owner and manager</h2>
        <span className="text-text-dim text-[12px]">
          stored as 20-byte lock-hash prefixes in the name&apos;s cell
        </span>
      </div>
      <div className="grid min-w-0 gap-4 sm:grid-cols-2">
        {cards.map((c) => (
          <dl
            key={c.label}
            aria-label={c.label}
            className={cn(
              'min-w-0 rounded-lg border px-4 py-3.5',
              c.accent ? 'border-identity/50' : 'border-base-border'
            )}
          >
            <NamedParty party={c.party} label={c.label} />
            <dd className="text-text-dim mt-1 text-[12px]">since {formatUtc8Date(c.since)}</dd>
          </dl>
        ))}
      </div>
      <h3 className="text-text-dim pt-1 text-[12px] font-normal">Owners so far</h3>
      <ol className="flex min-w-0 flex-col gap-2 sm:flex-row sm:items-stretch">
        {runs.map((run, i) => {
          const step = i === runs.length - 1 ? 'now' : i === 0 ? 'registered' : '';
          const party = run.since.decoded.owner;
          return (
            <li
              key={`${run.owner}:${run.since.cell.txHash}`}
              data-owner-step={step}
              className={cn(
                'flex min-w-0 flex-1 flex-col gap-0.5 rounded-md border px-3 py-2.5',
                step === 'now' ? 'border-identity/60' : 'border-base-border border-dashed'
              )}
            >
              <span className="text-text-dim text-[11.5px]">
                {formatUtc8Date(run.since.cell.timestamp)}
                {step ? ` · ${step}` : ''}
              </span>
              <span
                className={cn(
                  'truncate text-[12.5px]',
                  step === 'now' ? 'text-text-bright' : 'text-text'
                )}
              >
                {partyShort(party)}
              </span>
              <span className="text-text-dim text-[11.5px]">
                {party.scriptName ?? 'lock not named'}
              </span>
            </li>
          );
        })}
      </ol>
    </section>
  );
}
