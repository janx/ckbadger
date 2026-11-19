'use client';
import { useDetailTier } from '@/components/ui/detail-layout';
import type { MnftProtocol, DotCellPartyRef } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { CopyButton } from '@/components/ui/copy-button';
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
            <Hash hash={party.hashPrefix} truncate={false} className="text-meta" />{' '}
            {/* a full id on its own line in tier 2: the id is text, the icon after it copies */}
            <CopyButton
              value={party.hashPrefix}
              label="copy lock hash prefix"
              className="align-middle"
            />
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
          <div key={key} className="border-row-rule flex justify-between gap-3 border-b py-2">
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
