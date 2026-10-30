'use client';
import type { SporeDetail, SporeDobDecoded } from '@/lib/api';
import { SporePreview } from '@/components/object/spore-preview';
import { detectPreview } from '@/lib/preview-utils';
import { hexBytes } from '@/lib/hex-bytes';
import { bitcoinTxHref, omigaDobHref, parseBtcfsUri } from '@/lib/btcfs';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { CompositionBadge } from '@/components/ui/composition-mark';
export interface ObjectMediaProps {
  object: SporeDetail;
  contentHex: string | null;
  decoded?: SporeDobDecoded;
}
export function ObjectMedia({ object, contentHex, decoded }: ObjectMediaProps) {
  const network = useActiveNetwork();
  const omiga = omigaDobHref(object.typeScriptHash, network);
  const profile = object.mediaProfile;
  const bitcoin = profile?.sources.filter((s) => s.scheme === 'btcfs') ?? [];
  const bytes =
    contentHex === null
      ? undefined
      : Uint8Array.from(hexBytes(contentHex, object.sporeId), (b) => parseInt(b, 16));
  const preview = bitcoin.length ? null : detectPreview(object.contentType, bytes, decoded?.media);
  return (
    <section aria-label="Object media" className="min-w-0 space-y-4">
      {profile ? (
        <CompositionBadge tier={profile.tier} />
      ) : (
        <span className="text-text-dim text-[11px]">Composition not recorded</span>
      )}
      {bitcoin.length ? (
        <figure className="border-copper/40 space-y-3 border p-6 text-center">
          <div className="text-copper text-[18px]">Its image lives on Bitcoin</div>
          {bitcoin.map((s) => {
            const inscription = parseBtcfsUri(s.uri);
            const href = inscription && bitcoinTxHref(inscription.txid, network);
            const label =
              inscription &&
              `Bitcoin tx ${inscription.txid.slice(0, 4)}…${inscription.txid.slice(-4)} · inscription i${inscription.index}`;
            return (
              <div key={s.uri} className="min-w-0 space-y-2">
                <div className="text-text-dim break-all text-[11px]">{s.uri}</div>
                {inscription === null ? (
                  <p className="text-text-dim text-[11px]">
                    Not a recognisable btcfs inscription reference.
                  </p>
                ) : href ? (
                  <a
                    href={href}
                    className="text-copper break-all"
                    target="_blank"
                    rel="noopener noreferrer"
                  >
                    {label} ↗
                  </a>
                ) : (
                  <p className="space-y-1">
                    <span className="text-copper block break-all">{label}</span>
                    <span className="text-text-dim block text-[11px]">
                      The Bitcoin network of testnet inscriptions is not established, so no explorer
                      link is given.
                    </span>
                  </p>
                )}
              </div>
            );
          })}
          <figcaption className="text-text-dim text-[11px]">
            CKBadger reads CKB only. This object points to Bitcoin; its inscription is not fetched
            here.
          </figcaption>
        </figure>
      ) : preview ? (
        <div className="mx-auto max-w-[360px]">
          <SporePreview
            preview={preview}
            physicality={profile?.tier === 'pure_ckb' ? 'onchain' : 'default'}
          />
        </div>
      ) : (
        <p className="text-text-dim text-[11px]">
          {profile?.tier === 'decode_pending'
            ? 'Decode pending · awaiting DOB execution.'
            : 'No renderable media available.'}
        </p>
      )}
      {omiga && (
        <a
          href={omiga}
          className="text-text-dim hover:text-jade inline-block text-[11px]"
          target="_blank"
          rel="noopener noreferrer"
        >
          View on Omiga ↗
        </a>
      )}
      {profile && (
        <details className="min-w-0 text-[11px]">
          <summary className="cursor-pointer">
            Media dependencies · {profile.sources.length} sources
          </summary>
          <ul className="mt-3 space-y-3">
            {profile.sources.map((s, i) => (
              <li key={`${s.uri}:${i}`} className="break-all">
                <span className="text-text-dim">
                  {s.scheme} · {s.sourceLocation} · {s.dependencyTier}
                </span>
                <div>{s.uri}</div>
              </li>
            ))}
          </ul>
          {profile.issues.map((issue) => (
            <p key={issue} className="mt-2">
              {issue}
            </p>
          ))}
        </details>
      )}
    </section>
  );
}
