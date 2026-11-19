'use client';
import { columnPx, fontPx, textPx } from '@/lib/type-metrics';
import { HeadId } from '@/components/ui/head-id';
import { useDetailTier } from '@/components/ui/detail-layout';
import type { MnftProtocol, DotCellPartyRef } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
export function NamedParty({ party, label }: { party: DotCellPartyRef; label: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-meta">{label}</dt>
      <dd className="mt-1 [overflow-wrap:anywhere]">
        {party.lockHash ? (
          <AppLink look="inline" href={`/address/${party.address ?? party.lockHash}`}>
            <Hash
              hash={party.address ?? party.lockHash}
              kind={party.address ? 'address' : 'hash'}
              size="row"
              className="text-meta"
            />
          </AppLink>
        ) : (
          <>
            {/* Lane P2's follow-up (item 8): the 20-byte prefix as a head's id, its copy after it:
                42 characters at the line's 11 px are 277.2 px, one line in the tier's column at
                1280, 1024 and 640, and at 390 (358 less the copy's 40) and 360 (328 less 40) */}
            <HeadId value={party.hashPrefix} kind="hash" copy="copy lock hash prefix" />
            <span className="text-text-dim text-meta mt-1 block">
              Named lock-hash prefix · no uniquely resolved address
            </span>
          </>
        )}
        {party.scriptName && (
          <span className="text-text-dim text-meta block">{party.scriptName}</span>
        )}
      </dd>
    </div>
  );
}
export interface MnftPermissionsProps {
  permissions: MnftProtocol['permissions'];
  configure: number;
  state?: { claimed: boolean; locked: boolean; value: number };
}
export function MnftPermissions({ permissions, configure, state }: MnftPermissionsProps) {
  const labels: Record<keyof MnftProtocol['permissions'], string> = {
    allowClaim: 'Claim',
    allowLock: 'Lock',
    allowExtension: 'Extension',
    allowCharacteristicUpdate: 'Update characteristic',
    allowTransferBeforeClaim: 'Transfer before claim',
    allowTransferAfterClaim: 'Transfer after claim',
    allowDestroyBeforeClaim: 'Destroy before claim',
    allowDestroyAfterClaim: 'Destroy after claim',
  };
  // F2 objects (visual pass): in tier 2 (m-NFT item and class pages) the heading is tier-2 style.
  const tier = useDetailTier();
  return (
    <section aria-label="mNFT permissions" className="space-y-4">
      <h2
        className={tier === 2 ? 'text-body text-text font-medium' : 'text-text-bright text-section'}
      >
        What its configuration permits
      </h2>
      <p className="text-text text-meta max-w-measure">
        Configure 0x{configure.toString(16).padStart(2, '0')} · zero permission bits allow the
        operation.
      </p>
      <dl className="grid gap-3 sm:grid-cols-2">
        {Object.entries(labels).map(([key, label]) => (
          // lane P2's third round (item 4): the word at one gap (12 px) after its name, left-aligned,
          // the name column the longest name's at the meta step ("Update characteristic", 21
          // characters, 151.2 px: 152)
          <div
            key={key}
            className="border-row-rule grid min-w-0 items-baseline gap-x-3 border-b py-2"
            style={{
              gridTemplateColumns: `${columnPx(
                textPx(Math.max(...Object.values(labels).map((l) => l.length)), fontPx('meta'))
              )}px minmax(0, 1fr)`,
            }}
          >
            <dt className="text-meta">{label}</dt>
            <dd
              className={
                permissions[key as keyof typeof permissions] ? 'text-jade' : 'text-text-dim'
              }
            >
              {permissions[key as keyof typeof permissions] ? 'Allowed' : 'Forbidden'}
            </dd>
          </div>
        ))}
      </dl>
      {state && (
        <p className="text-meta max-w-measure">
          State 0x{state.value.toString(16).padStart(2, '0')} ·{' '}
          {state.claimed ? 'claimed' : 'unclaimed'} · {state.locked ? 'locked' : 'unlocked'}.{' '}
          {state.locked
            ? 'The locked state prevents transfer even when its configure permission allows it.'
            : 'These are configuration permissions, not a guarantee that a proposed operation satisfies every script condition.'}
        </p>
      )}
    </section>
  );
}
