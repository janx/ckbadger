import { AppLink } from '@/components/ui/app-link';
import {
  describeIdentityRecords,
  type IdentityRecordInput,
  type IdentityRecordValue,
} from '@/lib/identity-records';

function RecordValue({ value }: { value: IdentityRecordValue }) {
  switch (value.kind) {
    case 'text':
      return <span className="text-text-bright [overflow-wrap:anywhere]">{value.text}</span>;
    case 'colour':
      return (
        <span className="text-text-bright flex items-center gap-2">
          <span
            data-accent-swatch
            aria-hidden="true"
            className="h-3 w-3 shrink-0 rounded-[3px]"
            // The owner's own colour value, validated as #rgb / #rrggbb before it gets here.
            style={{ backgroundColor: value.text }}
          />
          {value.text}
        </span>
      );
    case 'bytes':
      return (
        <span className="text-text">{value.bytes.toLocaleString('en-US')} bytes, not text</span>
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
export function IdentityRecords({
  subject,
  records,
}: {
  subject: string;
  records: readonly IdentityRecordInput[];
}) {
  const view = describeIdentityRecords(records);
  const title = `What ${subject} points to`;
  return (
    <section aria-label={title} className="min-w-0 space-y-3">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 className="text-text-bright text-[15px] font-medium">{title}</h2>
        <span className="text-text-dim text-[12px]">its records</span>
      </div>
      {records.length === 0 ? (
        <p className="text-text">No records are set.</p>
      ) : (
        <>
          {view.addresses.length > 0 && (
            <dl className="flex flex-col">
              {view.addresses.map((a) => (
                <div
                  key={a.key}
                  className="border-base-border-subtle grid min-w-0 gap-1 border-t py-2.5 sm:grid-cols-[190px_minmax(0,1fr)] sm:gap-4"
                >
                  <dt className="text-text-dim">
                    <span>{a.coin}</span>
                    <span className="text-text-ghost block text-[11px]">
                      {a.key} · coin type {a.coinType}
                    </span>
                  </dt>
                  <dd className="min-w-0">
                    {a.ckbAddress ? (
                      <AppLink
                        href={`/address/${a.ckbAddress}`}
                        className="[overflow-wrap:anywhere]"
                      >
                        {a.ckbAddress}
                      </AppLink>
                    ) : (
                      <RecordValue value={a.value} />
                    )}
                  </dd>
                </div>
              ))}
            </dl>
          )}
          {view.profile.length > 0 && (
            <dl className="border-base-border-subtle grid min-w-0 grid-cols-1 gap-x-6 gap-y-3.5 border-t pt-3.5 sm:grid-cols-3">
              {view.profile.map((p) => (
                <div key={p.key} className="min-w-0">
                  <dt className="text-text-dim text-[11.5px]">{p.label}</dt>
                  <dd className="mt-0.5 min-w-0">
                    <RecordValue value={p.value} />
                  </dd>
                </div>
              ))}
            </dl>
          )}
          {view.other.length > 0 && (
            <dl className="border-base-border-subtle flex flex-col border-t pt-2">
              {view.other.map((o, i) => (
                <div
                  key={`${o.key}:${i}`}
                  className="grid min-w-0 gap-1 py-1.5 sm:grid-cols-[190px_minmax(0,1fr)] sm:gap-4"
                >
                  <dt className="text-text-dim [overflow-wrap:anywhere]">
                    {o.key || '(empty key)'}
                  </dt>
                  <dd className="min-w-0">
                    <RecordValue value={o.value} />
                  </dd>
                </div>
              ))}
            </dl>
          )}
          <p className="text-text-dim text-[12px]">
            {records.length.toLocaleString('en-US')} record{records.length === 1 ? '' : 's'} · each
            cached for {view.ttls.map((t) => `${t.toLocaleString('en-US')} s`).join(' or ')}
          </p>
        </>
      )}
    </section>
  );
}
