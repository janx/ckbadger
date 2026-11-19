'use client';
import type { SporeDetail, SporeDobDecoded } from '@/lib/api';
import { SporePreview } from '@/components/object/spore-preview';
import { detectPreview } from '@/lib/preview-utils';
import { hexBytes } from '@/lib/hex-bytes';
import { bitcoinTxHref, omigaDobHref, parseBtcfsUri, type BtcfsInscription } from '@/lib/btcfs';
import { inscriptionList } from '@/lib/dob-genes';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { ClassMark } from '@/components/ui/class-mark';
import { formatCount } from '@/lib/format-count';
import { ShortId } from '@/components/ui/short-id';
import { unitNode } from '@/components/ui/units';
import { NoBreak } from '@/components/ui/no-break';
export interface ObjectMediaProps {
  object: SporeDetail;
  contentHex: string | null;
  decoded?: SporeDobDecoded;
  /**
   * Whether the media column carries the object's link to Omiga — the "View image ↗" button (lane
   * U follow-up). Default true; false leaves it out.
   */
  omigaLink?: boolean;
}

/**
 * CKBadger does not render a DOB's picture itself (local first), so the way to see it sits where
 * the eye looks for the picture: one filled jade button centred in the media frame, its off-site
 * source said beneath (lane U follow-up; user: "'Omiga' should read 'Image View' or so, and be far
 * more visible").
 */
function ViewImage({ href }: { href: string }) {
  return (
    <div data-role="view-image" className="flex flex-col items-center gap-1.5">
      <a
        href={href}
        target="_blank"
        rel="noopener noreferrer"
        title="Opens the object's picture on Omiga in a new tab"
        className="bg-jade text-base-bg text-body focus-visible:outline-jade inline-flex h-9 items-center px-4 font-mono font-medium transition-opacity hover:opacity-90 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2"
      >
        View image ↗
      </a>
      <span className="text-text-dim text-meta">rendered off-site by Omiga</span>
    </div>
  );
}
/**
 * One inscription as the media box reads it: the txid at the row size (U-09; it was 4…4), in words
 * that break between them, never inside one (B3 round 3: break-all set "inscript / ion i0"); its
 * two units hold, "Bitcoin tx" with its id and "inscription i<n>". Linked to its explorer where
 * the network has one.
 */
function InscriptionReading({
  inscription,
  href,
}: {
  inscription: BtcfsInscription;
  href: string | null;
}) {
  const label = (
    <>
      <NoBreak>
        Bitcoin tx <ShortId value={inscription.txid} kind="hash" size="row" />
      </NoBreak>{' '}
      · <NoBreak>inscription i{inscription.index}</NoBreak>
    </>
  );
  return href ? (
    <a href={href} className="text-copper" target="_blank" rel="noopener noreferrer">
      {label} ↗
    </a>
  ) : (
    <span className="text-copper block">{label}</span>
  );
}

const NO_EXPLORER = (
  <span className="text-text-dim text-meta max-w-measure block">
    The Bitcoin network of testnet inscriptions is not established, so no explorer link is given.
  </span>
);

/**
 * What a btcfs reference points at, read from its URI: the Bitcoin transaction and inscription,
 * linked to an explorer where the network has one, else said in words. A `;`-list of inscriptions
 * (one value, the inscriptions an object can point at) is read one inscription each, through the
 * one path that decides what such a list is (`inscriptionList`, which traitDisplay counts: B3
 * round 4); a URI that is neither, a list with an element that is not an inscription included, is
 * said to be no inscription reference (anyone can mint spore content).
 */
function BtcfsReading({ uri, network }: { uri: string; network: string }) {
  const single = parseBtcfsUri(uri);
  const inscriptions = single ? [single] : inscriptionList(uri);
  if (inscriptions === null)
    return <p className="text-text text-meta">Not a recognisable btcfs inscription reference.</p>;
  const readings = inscriptions.map((inscription) => (
    <InscriptionReading
      key={`${inscription.txid}i${inscription.index}`}
      inscription={inscription}
      href={bitcoinTxHref(inscription.txid, network)}
    />
  ));
  const linked = bitcoinTxHref(inscriptions[0].txid, network) !== null;
  if (readings.length === 1)
    return linked ? (
      readings[0]
    ) : (
      <p className="max-w-measure space-y-1">
        {readings[0]}
        {NO_EXPLORER}
      </p>
    );
  const list = (
    <ul aria-label="Inscriptions" className="space-y-1">
      {readings.map((reading) => (
        <li key={reading.key}>{reading}</li>
      ))}
    </ul>
  );
  return linked ? (
    list
  ) : (
    <div className="space-y-1">
      {list}
      {NO_EXPLORER}
    </div>
  );
}

export function ObjectMedia({ object, contentHex, decoded, omigaLink = true }: ObjectMediaProps) {
  const network = useActiveNetwork();
  const omiga = omigaDobHref(object.typeScriptHash, network);
  const profile = object.mediaProfile;
  const bitcoin = profile?.sources.filter((s) => s.scheme === 'btcfs') ?? [];
  // PB-15: the box reads the first reference only, so its action stays under its title; the
  // further ones are read in the disclosure that follows the box.
  const [firstReference] = bitcoin;
  const bytes =
    contentHex === null
      ? undefined
      : Uint8Array.from(hexBytes(contentHex, object.sporeId), (b) => parseInt(b, 16));
  const preview = bitcoin.length ? null : detectPreview(object.contentType, bytes, decoded);
  return (
    <section aria-label="Object media" className="min-w-0 space-y-4">
      {/* F2 objects (OB-6): the composition badge and its sentence now open the page hero. */}
      {firstReference ? (
        // Lane P2's follow-up (item 2): where the picture comes from is a note after the way to
        // the picture, in the quiet ink, said once (a copper box with a 22 px title over the
        // facts, and a caption that said it again, made a missing picture the page's loudest
        // words); the first reference's inscription after it, its raw URI in the disclosure
        <figure className="m-0 min-w-0 space-y-3 text-center">
          {omigaLink && omiga && <ViewImage href={omiga} />}
          <figcaption className="text-meta max-w-measure mx-auto min-w-0 space-y-1">
            {/* a note under the figure, the figure's caption: quiet by rule (R3) */}
            <span className="text-text-dim text-meta block">
              Its image lives on Bitcoin; CKBadger reads CKB only and does not fetch it.
            </span>
            <BtcfsReading uri={firstReference.uri} network={network} />
          </figcaption>
        </figure>
      ) : preview ? (
        <div className="space-y-4">
          <div className="mx-auto max-w-[360px]">
            <SporePreview
              preview={preview}
              physicality={profile?.tier === 'pure_ckb' ? 'onchain' : 'default'}
            />
          </div>
          {omigaLink && omiga && <ViewImage href={omiga} />}
        </div>
      ) : (
        // Nothing renders here: a square placeholder with the class mark holds the place of the
        // picture, and the way to see it is its centre.
        <figure
          aria-label="No picture shown here"
          className="border-base-border m-0 flex aspect-square flex-col items-center justify-center gap-4 border p-6 text-center"
        >
          <ClassMark kind="objects" size={24} />
          <p className="text-text-dim text-meta max-w-measure">
            {profile?.tier === 'decode_pending'
              ? 'Decode pending · awaiting DOB execution.'
              : 'No renderable media available.'}
          </p>
          {omigaLink && omiga && <ViewImage href={omiga} />}
        </figure>
      )}
      {profile && (
        <details className="text-meta min-w-0">
          <summary className="cursor-pointer">
            Media dependencies · {unitNode(formatCount(profile.sources.length, 'source'))}
          </summary>
          <ul className="mt-3 space-y-3">
            {profile.sources.map((s, i) => (
              <li key={`${s.uri}:${i}`}>
                <span className="text-text-dim">
                  {s.scheme} · {s.sourceLocation} · {s.dependencyTier}
                </span>
                {/* only the raw URI breaks anywhere; the words around it break between them */}
                <div className="break-all">{s.uri}</div>
                {/* A further Bitcoin reference is read here, where the box no longer does. */}
                {s.scheme === 'btcfs' && s !== firstReference && (
                  <div className="mt-1">
                    <BtcfsReading uri={s.uri} network={network} />
                  </div>
                )}
              </li>
            ))}
          </ul>
          {profile.issues.map((issue) => (
            <p key={issue} className="max-w-measure mt-2">
              {issue}
            </p>
          ))}
        </details>
      )}
    </section>
  );
}
