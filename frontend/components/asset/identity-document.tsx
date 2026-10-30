'use client';
import { useDetailTier } from '@/components/ui/detail-layout';
import type { MnftProtocol, DotCellPartyRef } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
/** Keep binary profile blobs out of tier 1 (ID-DC2). */
export function truncateRecordHex(hex: string, head = 18, tail = 8): string {
  if (!hex.startsWith('0x')) throw new Error(`record hex missing 0x: ${hex.slice(0, 16)}`);
  if (hex.length <= 2 + head + tail) return hex;
  return `${hex.slice(0, 2 + head)}…${hex.slice(-tail)}`;
}
export function NamedParty({ party, label }: { party: DotCellPartyRef; label: string }) {
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-[11px]">{label}</dt>
      <dd className="mt-1 break-all">
        {party.lockHash ? (
          <AppLink href={`/address/${party.address ?? party.lockHash}`}>
            <Hash hash={party.address ?? party.lockHash} className="text-[11px]" copyable={false} />
          </AppLink>
        ) : (
          <>
            <Hash hash={party.hashPrefix} truncate={false} className="text-[11px]" />
            <span className="text-text-dim mt-1 block text-[11px]">
              Named lock-hash prefix · no uniquely resolved address
            </span>
          </>
        )}
        {party.scriptName && (
          <span className="text-text-dim block text-[11px]">{party.scriptName}</span>
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
        className={
          tier === 2 ? 'text-[13px] font-medium text-[#a0a8be]' : 'text-text-bright text-[15px]'
        }
      >
        What its configuration permits
      </h2>
      <p className="text-text-dim text-[11px]">
        Configure 0x{configure.toString(16).padStart(2, '0')} · zero permission bits allow the
        operation.
      </p>
      <dl className="grid gap-3 sm:grid-cols-2">
        {Object.entries(labels).map(([key, label]) => (
          <div key={key} className="border-base-border flex justify-between gap-3 border-b py-2">
            <dt className="text-[11px]">{label}</dt>
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
        <p className="text-[11px]">
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
