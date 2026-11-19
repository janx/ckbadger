import { AppLink } from '@/components/ui/app-link';
import { PromptHeading } from '@/components/ui/prompt-heading';
import {
  describeIdentityRecords,
  type IdentityRecordInput,
  type IdentityRecordValue,
} from '@/lib/identity-records';
import { formatCount, countNoun } from '@/lib/format-count';
import { unitNode } from '@/components/ui/units';

function RecordValue({ value, label }: { value: IdentityRecordValue; label?: string }) {
  switch (value.kind) {
    case 'image':
      // J13: the owner's own image bytes as a thumbnail, the exact size beside it.
      return (
        <span className="flex items-center gap-2.5">
          <img
            src={value.src}
            alt={`${label ?? 'Record'}, a ${value.format} image of ${formatCount(value.bytes, 'byte')}`}
            className="border-base-border h-12 w-12 shrink-0 border object-cover"
          />
          <span className="text-text">
            {value.format} · {unitNode(formatCount(value.bytes, 'byte'))}
          </span>
        </span>
      );
    case 'text':
      return <span className="text-text-bright [overflow-wrap:anywhere]">{value.text}</span>;
    case 'colour':
      return (
        <span className="text-text-bright flex items-center gap-2">
          <span
            data-accent-swatch
            aria-hidden="true"
            className="h-3 w-3 shrink-0"
            // The owner's own colour value, validated as #rgb / #rrggbb before it gets here.
            style={{ backgroundColor: value.text }}
          />
          {value.text}
        </span>
      );
    case 'bytes':
      return (
        <span className="text-text">{unitNode(formatCount(value.bytes, 'byte'))}, not text</span>
      );
    case 'empty':
      return <span className="text-text-dim">set, empty</span>;
  }
}

/**
 * "What <name> points to" (ID-DC5): the identity's records in plain words — addresses by coin,
 * the profile as a grid — with the stored keys kept beside them. The records as stored, byte
 * for byte, stay in tier 2.
 */
interface RecordParty {
  lockHash: string | null;
  lockScriptName: string | null;
}

/** "the manager's own JoyID wallet": an address record that is the owner's or manager's lock. */
function whose(lockHash: string | null, parties?: { owner: RecordParty; manager: RecordParty }) {
  if (!lockHash || !parties) return null;
  const owner = parties.owner.lockHash === lockHash,
    manager = parties.manager.lockHash === lockHash;
  const party = owner ? parties.owner : manager ? parties.manager : null;
  if (!party) return null;
  const lock = party.lockScriptName ? `${party.lockScriptName} ` : '';
  return `the ${owner ? 'owner' : 'manager'}'s own ${lock}wallet`;
}

export function IdentityRecords({
  subject,
  records,
  parties,
}: {
  subject: string;
  records: readonly IdentityRecordInput[];
  /** `.cell`: the named owner and manager, to say when an address record is one of theirs. */
  parties?: { owner: RecordParty; manager: RecordParty };
}) {
  const view = describeIdentityRecords(records);
  const title = `What ${subject} points to`;
  return (
    <section aria-label={title} className="min-w-0 space-y-3">
      {/* Rubbing × Song print, phase 5 (A5): the records, run on the name and their count; the
          command says what the old "its records" note did. */}
      <PromptHeading
        command="Records"
        args={`${subject} · ${records.length.toLocaleString('en-US')}`}
      />
      {records.length === 0 ? (
        <p className="text-text">No records.</p>
      ) : (
        <>
          {view.addresses.length > 0 && (
            <dl className="flex flex-col">
              {view.addresses.map((a) => (
                <div
                  key={a.key}
                  className="border-row-rule grid min-w-0 gap-1 border-t py-2.5 sm:grid-cols-[190px_minmax(0,1fr)] sm:gap-4"
                >
                  <dt className="text-text-dim">
                    <span>{a.coin}</span>
                    <span className="text-text-dim text-meta block">
                      {a.key} · coin type {a.coinType}
                    </span>
                  </dt>
                  <dd className="min-w-0">
                    {a.ckbAddress ? (
                      <AppLink
                        look="inline"
                        href={`/address/${a.ckbAddress}`}
                        className="[overflow-wrap:anywhere]"
                      >
                        {a.ckbAddress}
                      </AppLink>
                    ) : (
                      <RecordValue value={a.value} />
                    )}
                    {whose(a.ckbLockHash, parties) && (
                      <span className="text-text-dim text-small block">
                        {whose(a.ckbLockHash, parties)}
                      </span>
                    )}
                  </dd>
                </div>
              ))}
            </dl>
          )}
          {view.profile.length > 0 && (
            <dl className="border-row-rule grid min-w-0 grid-cols-1 gap-x-6 gap-y-3.5 border-t pt-3.5 sm:grid-cols-3">
              {view.profile.map((p) => (
                <div key={p.key} className="min-w-0">
                  <dt className="text-text-dim text-meta">{p.label}</dt>
                  <dd className="mt-0.5 min-w-0">
                    <RecordValue value={p.value} label={p.label} />
                  </dd>
                </div>
              ))}
            </dl>
          )}
          {view.other.length > 0 && (
            <dl className="border-row-rule flex flex-col border-t pt-2">
              {view.other.map((o, i) => (
                <div
                  key={`${o.key}:${i}`}
                  className="grid min-w-0 gap-1 py-1.5 sm:grid-cols-[190px_minmax(0,1fr)] sm:gap-4"
                >
                  <dt className="text-text-dim [overflow-wrap:anywhere]">
                    {o.key || '(empty key)'}
                  </dt>
                  <dd className="min-w-0">
                    <RecordValue value={o.value} label={o.key} />
                  </dd>
                </div>
              ))}
            </dl>
          )}
          <p className="text-text text-small max-w-measure">
            {records.length.toLocaleString('en-US')} {countNoun(records.length, 'record')} · each
            cached for {view.ttls.map((t) => `${t.toLocaleString('en-US')} s`).join(' or ')}
          </p>
        </>
      )}
    </section>
  );
}
