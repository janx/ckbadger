'use client';
import type { ReactNode } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, type DotCellItem, type ItemProtocol, type ProtocolVersion } from '@/lib/api';
import { AppLink } from '@/components/ui/app-link';
import { Hash } from '@/components/ui/hash';
import { useDetailTier } from '@/components/ui/detail-layout';
import { AssetReadState } from '@/components/asset/detail-shell';
import { truncateRecordHex } from '@/components/asset/identity-document';
import type { IdentityStandard } from '@/components/identity/identity-header';
import { formatUtc8DateTime, cn } from '@/lib/utils';
import { retryTransientOnly } from '@/lib/query-client';

type Of<K extends ItemProtocol['kind']> = Extract<ItemProtocol, { kind: K }>;

function byteLength(hex: string, what: string) {
  if (!hex.startsWith('0x') || hex.length % 2 !== 0) throw new Error(`${what}: invalid data hex`);
  return (hex.length - 2) / 2;
}

/** The data map's title (ID-C9): what the cell is and its size, not the protocol slug. */
export function identityDataTitle(standard: IdentityStandard) {
  return (version: ProtocolVersion<unknown>): string => {
    const bytes = byteLength(version.dataHex, `${version.cell.txHash}:${version.cell.outputIndex}`);
    switch (standard) {
      case 'dotbit':
        return `Account cell data · ${bytes} bytes`;
      case 'dotcell':
        return `Name cell data · layout v${(version.decoded as { layoutVersion: number }).layoutVersion} · ${bytes} bytes`;
      case 'bit_cell':
      case 'did_ckb':
        return `Cell data · ${bytes} bytes`;
    }
  };
}

/**
 * A tier-2 section: its heading quiet inside tier 2 (FINAL_REVIEW, ID-C12), named for the outline.
 * `empty` (consistency C16): the section has nothing to show, said in the heading's own line
 * ("Records as stored · witness · none in this version") instead of a heading over a "No …" line.
 */
function Tier2Section({
  title,
  empty,
  children,
}: { title: string } & (
  | { empty: string; children?: never }
  | { empty?: never; children: ReactNode }
)) {
  const tier = useDetailTier();
  return (
    <section aria-label={title} className="min-w-0 space-y-3">
      <h2
        className={cn(
          'font-medium',
          tier === 2 ? 'text-text text-body' : 'text-text-bright text-section'
        )}
      >
        {title}
        {empty !== undefined && (
          <span className="text-text-dim text-small font-normal"> · {empty}</span>
        )}
      </h2>
      {children}
    </section>
  );
}

function Rows({ rows }: { rows: [string, ReactNode][] }) {
  return (
    <dl className="text-small grid min-w-0 grid-cols-1 gap-x-3.5 gap-y-2 sm:grid-cols-[200px_minmax(0,1fr)]">
      {rows.map(([term, value]) => (
        <div key={term} className="contents">
          <dt className="text-text-dim">{term}</dt>
          <dd className="min-w-0 [overflow-wrap:anywhere]">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

const Dim = ({ children }: { children: ReactNode }) => (
  <span className="text-text-dim">{children}</span>
);

/** One field of a witness row: its value, the witness's own field name as its hover. */
function Field({ name, label, children }: { name: string; label?: string; children: ReactNode }) {
  return (
    <span title={name} className="whitespace-nowrap">
      {label !== undefined && <Dim>{label} </Dim>}
      {children}
    </span>
  );
}

/** Items of one row, a dim " · " between them. */
function Items({ items }: { items: ReactNode[] }) {
  return (
    <>
      {items.map((item, i) => (
        <span key={i}>
          {i > 0 && <Dim> · </Dim>}
          {item}
        </span>
      ))}
    </>
  );
}

/** A key the witness stores, with the algorithm that verifies it as a tag (owner, manager). */
function WitnessKey({ k, note }: { k: { algorithm: number; argsHex: string }; note?: ReactNode }) {
  return (
    <>
      <span
        data-role="algorithm"
        className="text-meta text-text-dim border-base-border whitespace-nowrap rounded-sm border px-[5px] leading-4"
      >
        algorithm {k.algorithm}
      </span>{' '}
      <span className="[overflow-wrap:anywhere]">{k.argsHex}</span>
      {note}
    </>
  );
}

/**
 * `.bit` account internals, decoded from the witness (ID-B1 / ID-B2; lane P): one designed block
 * of grouped 32 px rows — the term on one line, the witness's last changes, status and witness
 * version, the owner and manager keys with their algorithm tags, records and sub-account flags,
 * the account and the next one. Keys are `text-meta`, values `text-small`; the witness's own field
 * names (`registered_at`, `last_edit_manager_at`, …) are each value's hover, nothing dropped.
 */
export function DotbitAccountData({ protocol }: { protocol: Of<'dotbit'> }) {
  const v = protocol.versions.at(-1);
  if (!v) throw new Error(`${protocol.itemId}: .bit has no versions`);
  const d = v.decoded;
  const same = d.owner.algorithm === d.manager.algorithm && d.owner.argsHex === d.manager.argsHex;
  const changes = witnessChanges(d);
  const flags = subAccountFlags(d);
  const rows: { key: string; label: string; value: ReactNode }[] = [
    {
      key: 'term',
      label: 'registered → expires',
      value: (
        <>
          <Field name="registered_at" label="registered">
            {formatUtc8DateTime(Number(d.registeredAt) * 1000)}
          </Field>
          <Dim> → </Dim>
          <Field name="expired_at" label="expires">
            {d.expiredAt === null ? (
              <Dim>not recorded</Dim>
            ) : (
              formatUtc8DateTime(Number(d.expiredAt) * 1000)
            )}
          </Field>
        </>
      ),
    },
    ...(changes.length
      ? [{ key: 'changes', label: 'last changes', value: <Items items={changes} /> }]
      : []),
    {
      key: 'status',
      label: 'status · witness',
      value: (
        <Items
          items={[
            <Field key="s" name="status">
              0x{d.status.toString(16).padStart(2, '0')} <Dim>raw byte, as stored</Dim>
            </Field>,
            <Field key="w" name="witness version">
              <Dim>witness version</Dim> {d.witnessVersion}
            </Field>,
          ]}
        />
      ),
    },
    { key: 'owner', label: 'owner key', value: <WitnessKey k={d.owner} /> },
    {
      key: 'manager',
      label: 'manager key',
      value: <WitnessKey k={d.manager} note={same && <Dim> · the same as the owner</Dim>} />,
    },
    {
      key: 'records',
      label: 'records · sub-accounts',
      value: (
        <Items
          items={[
            <Field key="r" name="records">
              {d.records.length} {d.records.length === 1 ? 'entry' : 'entries'}
            </Field>,
            ...flags,
          ]}
        />
      ),
    },
    {
      key: 'account',
      label: 'account → next',
      value: (
        <>
          <Field name="account id">
            <span className="[overflow-wrap:anywhere]">{d.accountId}</span>
          </Field>
          <Dim> → </Dim>
          <Field name="next" label="next">
            {d.nextAccountId === null ? (
              <Dim>none</Dim>
            ) : (
              <AppLink href={`/identities/dotbit/${d.nextAccountId}`}>
                {d.nextAccountName ?? d.nextAccountId}
              </AppLink>
            )}
          </Field>
        </>
      ),
    },
  ];
  return (
    <Tier2Section title="Account data, from the witness">
      <div className="min-w-0">
        {rows.map((row) => (
          <div
            key={row.key}
            data-row={row.key}
            className="border-base-border-subtle grid min-h-8 min-w-0 grid-cols-1 items-center gap-x-4 border-t py-1.5 sm:grid-cols-[168px_minmax(0,1fr)]"
          >
            <span data-role="row-key" className="text-meta text-text-dim">
              {row.label}
            </span>
            <span data-role="row-value" className="text-small text-text min-w-0">
              {row.value}
            </span>
          </div>
        ))}
      </div>
      <p className="text-text-ghost text-meta">
        Decoded from the account data in the witness of tx{' '}
        <AppLink href={`/tx/${v.cell.txHash}`}>
          <Hash hash={v.cell.txHash} copyable={false} className="text-meta" />
        </AppLink>
      </p>
    </Tier2Section>
  );
}

function witnessTime(value: string | null): ReactNode {
  if (value === null) return <Dim>not in this witness version</Dim>;
  if (value === '0') return 'never';
  return formatUtc8DateTime(Number(value) * 1000);
}

/** R2b-b-17: items only for the fields the API serves; "0" is a time never set. */
function witnessChanges(d: Of<'dotbit'>['versions'][number]['decoded']): ReactNode[] {
  const items: ReactNode[] = [];
  for (const [name, label, value] of [
    ['last_transfer_account_at', 'transfer', d.lastTransferAccountAt],
    ['last_edit_manager_at', 'manager edit', d.lastEditManagerAt],
    ['last_edit_records_at', 'records edit', d.lastEditRecordsAt],
  ] as const)
    if (value !== undefined)
      items.push(
        <Field key={name} name={name} label={label}>
          {witnessTime(value)}
        </Field>
      );
  if (typeof d.updatedAt === 'string')
    items.push(
      <Field key="updated_at" name="updated_at" label="updated">
        {witnessTime(d.updatedAt)}
      </Field>
    );
  return items;
}

function subAccountFlags(d: Of<'dotbit'>['versions'][number]['decoded']): ReactNode[] {
  const items: ReactNode[] = [];
  if (d.enableSubAccount !== undefined)
    items.push(
      <Field key="e" name="enable_sub_account" label="sub-accounts">
        {d.enableSubAccount === null ? (
          <Dim>not in this witness version</Dim>
        ) : (
          String(d.enableSubAccount)
        )}
      </Field>
    );
  if (d.renewSubAccountPrice !== undefined)
    items.push(
      <Field key="p" name="renew_sub_account_price" label="renew price">
        {d.renewSubAccountPrice === null ? (
          <Dim>not in this witness version</Dim>
        ) : (
          <>
            {BigInt(d.renewSubAccountPrice).toLocaleString('en-US')} <Dim>raw</Dim>
          </>
        )}
      </Field>
    );
  return items;
}

/** The `.bit` account a `.bit Cell` names, as it is now (ID-BC2: its state at minting is not served). */
export function BitCellAccountNow({ protocol }: { protocol: Of<'bit_cell'> }) {
  const d = protocol.versions.at(-1)?.decoded;
  if (!d) throw new Error(`${protocol.itemId}: .bit Cell has no versions`);
  const a = d.linkedAccount;
  return (
    <Tier2Section title="The .bit account it names, now">
      <Rows
        rows={[
          ['account', d.account],
          ['account id', d.accountId],
          ...(a
            ? ([
                [
                  'account now',
                  <>
                    <AppLink href={`/identities/dotbit/${a.id}`}>{a.name ?? a.id}</AppLink>
                    <Dim> · {a.isLive ? 'live' : 'not live'}</Dim>
                  </>,
                ],
                [
                  'registered',
                  a.registeredAt === null ? (
                    <Dim>not recorded</Dim>
                  ) : (
                    formatUtc8DateTime(Number(a.registeredAt) * 1000)
                  ),
                ],
                [
                  'expires',
                  a.expiredAt === null ? (
                    <Dim>not recorded</Dim>
                  ) : (
                    formatUtc8DateTime(Number(a.expiredAt) * 1000)
                  ),
                ],
              ] as [string, ReactNode][])
            : ([['account now', <Dim key="none">not indexed</Dim>]] as [string, ReactNode][])),
        ]}
      />
    </Tier2Section>
  );
}

/** The did:ckb document as stored (ID-D4): the DID, its local ID, the data layout and the CBOR. */
export function DidStored({ protocol }: { protocol: Of<'did_ckb'> }) {
  const d = protocol.versions.at(-1)?.decoded;
  if (!d) throw new Error(`${protocol.itemId}: did:ckb has no versions`);
  const bytes = byteLength(d.documentCborHex, `${protocol.itemId} document`);
  return (
    <Tier2Section title={`Document as stored · CBOR, ${bytes.toLocaleString('en-US')} bytes`}>
      <Rows
        rows={[
          ['DID', d.did],
          ['local ID', d.localId ?? <Dim>not set</Dim>],
          ['data layout', `V${d.documentVersion} (union variant 0)`],
        ]}
      />
      <pre className="bg-base-elevated text-text text-meta whitespace-pre-wrap rounded px-3.5 py-3 [overflow-wrap:anywhere]">
        {JSON.stringify(d.document, null, 2)}
      </pre>
    </Tier2Section>
  );
}

const ZERO_ID = /^0x0{40}$/;

/**
 * `.cell` evidence (ID-DC6 / ID-DC7): the records exactly as the witness stores them, and the
 * name cell's namespace, shared lock and ring successor — all out of tier 1.
 */
export function DotcellStored({ protocol, item }: { protocol: Of<'dotcell'>; item: DotCellItem }) {
  const v = protocol.versions.at(-1);
  if (!v) throw new Error(`${protocol.itemId}: .cell has no versions`);
  const d = v.decoded;
  const next = useQuery({
    queryKey: ['dotcell-item-detail', d.nextId],
    queryFn: () => api.getDotCellItemDetail(d.nextId),
    enabled: !ZERO_ID.test(d.nextId),
    retry: retryTransientOnly,
  });
  return (
    <>
      {d.records.length === 0 ? (
        <Tier2Section title="Records as stored · witness" empty="none in this version" />
      ) : (
        <Tier2Section title="Records as stored · witness">
          <div className="text-small">
            {d.records.map((r, i) => (
              <div
                key={`${r.key}:${i}`}
                className="border-base-border-subtle grid min-w-0 grid-cols-[minmax(0,160px)_70px_minmax(0,1fr)] gap-3 border-t py-1.5"
              >
                <span className="[overflow-wrap:anywhere]">{r.key || '(empty key)'}</span>
                <span className="text-text-dim">ttl {r.ttl}</span>
                <span className="text-text min-w-0 [overflow-wrap:anywhere]">
                  {truncateRecordHex(r.valueHex)}
                </span>
              </div>
            ))}
          </div>
        </Tier2Section>
      )}
      <Tier2Section title="Name cell">
        <Rows
          rows={[
            [
              `Type · ${v.cell.typeName ?? 'not named'}`,
              <>
                {item.namespaceArgs} <Dim>· the namespace, the same for every name</Dim>
              </>,
            ],
            [
              `Lock · ${v.cell.lockName ?? 'not named'}`,
              <>
                <AppLink href={`/address/${v.cell.ownerAddress}`}>
                  <Hash hash={v.cell.ownerAddress} copyable={false} className="text-small" />
                </AppLink>{' '}
                <Dim>
                  · names are held under this lock; ownership is the owner field in the data
                </Dim>
              </>,
            ],
            ['Name id', item.identityId],
            ['Records hash', d.recordsHash],
            [
              'Next in the ring',
              ZERO_ID.test(d.nextId) ? (
                <Dim>the ring&apos;s end</Dim>
              ) : next.data ? (
                <AppLink href={`/identities/dotcell/${next.data.identityId}`}>
                  {next.data.name}
                </AppLink>
              ) : (
                <>
                  <AppLink href={`/identities/dotcell/${d.nextId}`}>
                    <Hash hash={d.nextId} copyable={false} className="text-small" />
                  </AppLink>
                  <AssetReadState label="its name" error={next.error} loading={next.isLoading} />
                </>
              ),
            ],
          ]}
        />
      </Tier2Section>
    </>
  );
}
