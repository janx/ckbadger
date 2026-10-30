'use client';

import { useState } from 'react';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import { Chart } from '@/components/ui/chart';
import type { ChartResponse, DaoEvidence, DaoStatistics } from '@/lib/api';
import { formatPercent, parseCkbDecimal, parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { DaoSources } from '@/components/dao/dao-sources';
import { DaoHistory } from '@/components/dao/dao-history';
import { formatAccumulatedRate } from '@/lib/dao-header';
import { exactUnsignedInteger, formatExactDecimal } from '@/lib/exact-integer';
import { formatCapacity, formatDurationMs, formatUtc8DateTime } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';

/** R8: every chart response carries the FIRST point holding its greatest value (null when empty). */
export interface ChartPeakPoint {
  date: string;
  value: string;
}
/** `/dao/charts/total-deposit`; `peak` is optional in the type only until `ChartResponse` names it. */
export type DaoDepositHistory = ChartResponse & { peak?: ChartPeakPoint | null };

interface Props {
  statistics: DaoStatistics;
  evidence: DaoEvidence;
  depositedHistory?: DaoDepositHistory;
  circulationHistory?: ChartResponse;
  historyError?: boolean;
}

function Amount({ value, signed = false }: { value: string; signed?: boolean }) {
  const amount = parseShannons(value, 'DAO amount');
  const parts = splitCkbAmount(amount);
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      {signed ? parts.sign : null}
      {parts.integer}
      <span className="text-text-dim">{parts.fraction}</span>
      <span className="text-text-dim/50">{parts.ghost}</span>
    </span>
  );
}

/** Chart / summary values arrive as CKB decimal strings, not shannons. */
function CkbDecimal({ value, signed = false }: { value: string; signed?: boolean }) {
  const parts = splitCkbAmount(parseCkbDecimal(value));
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      {signed ? parts.sign : null}
      {parts.integer}
      <span className="text-text-dim">{parts.fraction}</span>
      <span className="text-text-dim/50">{parts.ghost}</span>
    </span>
  );
}

/** Header AR integers (×10¹⁶) as the exact rate, the same form the block page decodes. */
function formatAr(raw: string): string {
  if (!/^\d+$/.test(raw)) throw new Error(`DAO AR "${raw}" is not an unsigned integer`);
  return formatAccumulatedRate(BigInt(raw));
}

function Note({ children }: { children: ReactNode }) {
  return <span className="text-text-dim font-mono text-[11px]">{children}</span>;
}

function Section({ title, note, children }: { title: string; note?: string; children: ReactNode }) {
  return (
    <section className="border-base-border border-t pt-5" aria-label={title}>
      <div className="mb-4 flex flex-wrap items-baseline justify-between gap-2">
        <h2 className="text-text-bright text-[15px] font-semibold">{title}</h2>
        {note && <span className="text-text-dim font-mono text-[11px]">{note}</span>}
      </div>
      {children}
    </section>
  );
}

function Metric({
  label,
  children,
  detail,
  className,
}: {
  label: string;
  children: ReactNode;
  detail?: ReactNode;
  className?: string;
}) {
  return (
    <div className={`min-w-0 ${className ?? ''}`}>
      <dt className="text-text-dim text-xs">{label}</dt>
      <dd className="text-text-bright mt-1 text-[18px] leading-tight">{children}</dd>
      {detail && <dd className="text-text-dim mt-1 text-[11.5px]">{detail}</dd>}
    </div>
  );
}

/**
 * The hero (board V63): deposited, its share of circulation on the last complete day and its peak,
 * then the metrics. Deposited, deposits and depositors are the evidence anchor's phase-0 facts;
 * Estimated APC and the 24 h change are the statistics anchor's.
 */
function Hero({
  statistics,
  evidence,
  deposited,
  ratio,
}: {
  statistics: DaoStatistics;
  evidence: DaoEvidence;
  deposited?: DaoDepositHistory;
  ratio?: ChartResponse;
}) {
  if (deposited && deposited.peak === undefined)
    throw new Error('/dao/charts/total-deposit has no peak (R8 chart contract)');
  const share = ratio?.data.at(-1);
  const peak = deposited?.peak ?? null;
  const change = statistics.depositChange24h;
  const changeShannons = change === null ? null : parseCkbDecimal(change);
  const year = evidence.ar.year365;
  return (
    <section
      aria-label="Nervos DAO summary"
      className="grid gap-8 lg:grid-cols-[minmax(0,1fr)_460px] lg:items-end lg:gap-12"
    >
      <div className="flex min-w-0 flex-col gap-1">
        <span className="text-text-dim text-xs">deposited</span>
        <span className="text-text-bright text-[30px] font-medium leading-tight">
          <Amount value={evidence.phase0.capacity} />{' '}
          <span className="text-text-dim text-[14px]">CKB</span>
        </span>
        {share && deposited && (
          <span className="text-text-dim text-[13px]">
            <span className="text-gold">{ratioText(share.value)}%</span> of circulating CKB on{' '}
            {share.date}
            {peak && (
              <>
                {' · '}peak{' '}
                <span className="text-text">
                  <CkbDecimal value={peak.value} />
                </span>{' '}
                CKB on {peak.date}
              </>
            )}
          </span>
        )}
      </div>
      <dl className="m-0 grid grid-cols-2 gap-x-7 gap-y-[18px]">
        <Metric label="Estimated APC" detail="at today's issuance and deposits">
          {statistics.estimatedApc}%
        </Metric>
        <Metric
          label="Compensation, last 365 days"
          detail={
            year ? "on free capacity, from the header's AR" : 'the chain is younger than 365 days'
          }
        >
          {year ? `${year.observedGrowthPercent}%` : '—'}
        </Metric>
        <Metric label="Depositors">
          {evidence.concentration.currentDepositors.toLocaleString('en-US')}
        </Metric>
        <Metric
          label="Deposits"
          detail={`+ ${evidence.phase1.count.toLocaleString('en-US')} withdrawals requested`}
        >
          {evidence.phase0.count.toLocaleString('en-US')}
        </Metric>
        <Metric label="Change in 24 h" className="col-span-2">
          {changeShannons === null ? (
            <span className="text-text-dim">—</span>
          ) : (
            <span
              data-role="change24h"
              className={
                changeShannons > BigInt(0)
                  ? 'text-positive'
                  : changeShannons < BigInt(0)
                    ? 'text-negative'
                    : 'text-text'
              }
            >
              <CkbDecimal value={change!} signed />
            </span>
          )}{' '}
          <span className="text-text-dim text-[12px]">CKB</span>
        </Metric>
      </dl>
    </section>
  );
}

function History({
  deposited,
  ratio,
  error,
}: {
  deposited?: ChartResponse;
  ratio?: ChartResponse;
  error?: boolean;
}) {
  return (
    <section aria-label="History" className="min-w-0">
      {error ? (
        <>
          <h2 className="text-text-bright mb-3 text-[15px] font-medium">History</h2>
          <div role="alert" className="text-rouge text-xs">
            DAO daily history could not be loaded from indexed snapshots.
          </div>
        </>
      ) : !deposited || !ratio ? (
        <>
          <h2 className="text-text-bright mb-3 text-[15px] font-medium">History</h2>
          <div className="text-text-dim text-xs">Loading canonical daily history…</div>
        </>
      ) : (
        <DaoHistory deposited={deposited} ratio={ratio} />
      )}
    </section>
  );
}

/** An exact millisecond string from the API as a safe integer for duration display. */
function exactSafeMs(value: string, context: string): number {
  const ms = exactUnsignedInteger(value, context);
  if (ms > BigInt(Number.MAX_SAFE_INTEGER))
    throw new Error(`${context}: ${value} ms is out of range`);
  return Number(ms);
}

/** The circulation ratio exactly as the API states it (a decimal percentage). */
function ratioText(value: string): string {
  return formatExactDecimal(value, 'DAO circulation ratio');
}

function Issuance({ statistics }: Pick<Props, 'statistics'>) {
  const mining = parseShannons(statistics.miningReward, 'miningReward');
  const compensation = parseShannons(statistics.depositCompensation, 'depositCompensation');
  const burnt = parseShannons(statistics.burnt, 'burnt');
  const claimed = parseShannons(statistics.totalCompensationPaid, 'totalCompensationPaid');
  const unclaimed = parseShannons(statistics.unclaimedCompensation, 'unclaimedCompensation');
  if (claimed + unclaimed !== compensation)
    throw new Error('DAO compensation parts do not equal total compensation');
  const total = mining + compensation + burnt;
  const issuanceEmpty = total === BigInt(0);
  const issued = parseShannons(statistics.tipTotalIssuance, 'DAO C at statistics anchor');
  const occupied = parseShannons(
    statistics.tipProtocolOccupiedCapacity,
    'DAO U at statistics anchor'
  );
  const depositorWeight = parseShannons(
    statistics.depositorAllocationWeight,
    'depositorAllocationWeight'
  );
  const other = issued - occupied - depositorWeight;
  if (
    issued <= BigInt(0) ||
    occupied < BigInt(0) ||
    depositorWeight < BigInt(0) ||
    other < BigInt(0)
  ) {
    throw new Error(
      `invalid DAO capacity-use partition at statistics block ${statistics.tipBlockNumber}`
    );
  }
  const parts = [
    { label: 'To miners', amount: mining, color: '#7a879e', note: 'occupied capacity share' },
    {
      label: 'Compensation for depositors',
      amount: compensation,
      color: '#f2c55c',
      note: 'via accumulated rate',
    },
    { label: 'Burnt', amount: burnt, color: '#667080', note: 'gone for good' },
  ];
  return (
    <Section
      title="Where secondary issuance goes"
      note={`since genesis · exact shannons at block ${formatBlockNumber(statistics.tipBlockNumber)}`}
    >
      <div className="mb-3 text-[13px]">
        Secondary issuance since genesis: <Amount value={total.toString()} /> CKB
      </div>
      {issuanceEmpty ? (
        <div className="text-text-dim text-[12px]">
          No secondary issuance has accrued at this indexed block; allocation shares are undefined.
        </div>
      ) : (
        <div className="flex h-7 w-full overflow-hidden" aria-label="Secondary issuance partition">
          {parts.map((part) => (
            <div
              key={part.label}
              style={{ width: `${formatPercent(part.amount, total, 3)}%`, background: part.color }}
              title={`${part.label}: ${formatPercent(part.amount, total, 2)}%`}
            />
          ))}
        </div>
      )}
      <div className="mt-3 grid gap-3 md:grid-cols-3">
        {parts.map((part) => (
          <div key={part.label} className="border-base-border border-l pl-3 text-[12px]">
            <div className="text-text-dim">
              {part.label} · {issuanceEmpty ? '—' : `${formatPercent(part.amount, total, 2)}%`}
            </div>
            <div className="text-text-bright">
              <Amount value={part.amount.toString()} /> CKB
            </div>
            <div className="text-text-dim text-[11px]">{part.note}</div>
          </div>
        ))}
      </div>
      <div className="border-base-border mt-5 grid gap-3 border-t pt-4 text-[12px] md:grid-cols-2">
        <div>
          Paid at completed withdrawals: <Amount value={statistics.totalCompensationPaid} /> CKB
        </div>
        <div>
          Still owed to live DAO cells: <Amount value={statistics.unclaimedCompensation} /> CKB
        </div>
      </div>
      <p className="text-text-dim mt-3 text-[12px]">
        Each block splits its secondary issuance by capacity use: the protocol occupied share goes
        to miners; the depositors&apos; weight, phase-0 free capacity plus the compensation it has
        already accrued, goes to DAO depositors (withdrawal requests are frozen and take no share);
        the rest is burnt and cannot be claimed later.
      </p>
      <div className="mt-4">
        <div className="text-text-dim mb-1 text-[12px]">
          Current capacity use at statistics block {formatBlockNumber(statistics.tipBlockNumber)}
        </div>
        <div className="flex h-3 w-full" aria-label="Current capacity use partition">
          {[
            { key: 'Protocol occupied', value: occupied, color: '#58a5b5' },
            { key: 'DAO depositors', value: depositorWeight, color: '#f2c55c' },
            { key: 'Other', value: other, color: '#667080' },
          ].map((part) => (
            <div
              key={part.key}
              title={`${part.key}: ${formatCapacity(part.value)}`}
              style={{ width: `${formatPercent(part.value, issued, 4)}%`, background: part.color }}
            />
          ))}
        </div>
        <div className="text-text-dim mt-1 flex flex-wrap gap-x-5 text-[11px]">
          <span>Protocol occupied {formatPercent(occupied, issued, 2)}%</span>
          <span>DAO depositors {formatPercent(depositorWeight, issued, 2)}%</span>
          <span>Other {formatPercent(other, issued, 2)}%</span>
        </div>
        <p className="text-text-dim mt-1 text-[11px]">
          Protocol occupied includes the genesis virtual occupied baseline; it is not the real
          occupied size of live cells.
        </p>
      </div>
      <Link
        href={`/blocks/${statistics.tipBlockNumber}`}
        className="mt-2 inline-block text-[11px] text-[#f2c55c] hover:underline"
      >
        Verify statistics anchor block {formatBlockNumber(statistics.tipBlockNumber)} →
      </Link>
    </Section>
  );
}

function Windows({ evidence }: { evidence: DaoEvidence }) {
  const [selectedDay, setSelectedDay] = useState<number | null>(null);
  const max = evidence.windows.reduce((largest, row) => {
    const value = parseShannons(row.capacity, 'window capacity');
    return value > largest ? value : largest;
  }, BigInt(0));
  return (
    <Section title="When deposits can leave" note={`at indexed epoch ${evidence.tipEpoch.display}`}>
      <p className="text-text-dim mb-4 max-w-4xl text-[13px]">
        A deposit runs in 180-epoch cycles from its creation epoch. Each of the 30 bars covers six
        exact epochs (about one nominal day), measured from the indexed tip. These are epoch ranges,
        not predicted calendar dates.
      </p>
      <div
        className="grid grid-cols-10 gap-1 sm:grid-cols-[repeat(30,minmax(0,1fr))]"
        role="list"
        aria-label="Phase-0 cycle boundaries"
      >
        {evidence.windows.map((row) => {
          const value = parseShannons(row.capacity, 'window capacity');
          const height = max === BigInt(0) ? 0 : Number((value * BigInt(100)) / max);
          return (
            <div
              key={row.nominalDay}
              role="listitem"
              tabIndex={0}
              aria-label={`Nominal day ${row.nominalDay}, epochs +${row.fromEpochOffset} to +${row.throughEpochOffset}: ${row.count} deposits, ${row.capacity} shannons`}
              onFocus={() => setSelectedDay(row.nominalDay)}
              onMouseEnter={() => setSelectedDay(row.nominalDay)}
              className="group flex h-28 flex-col justify-end focus:outline focus:outline-[#f2c55c]"
              title={`Nominal day ${row.nominalDay}, epochs +${row.fromEpochOffset} to +${row.throughEpochOffset}: ${row.count} deposits, ${row.capacity} shannons`}
            >
              <div
                className="w-full bg-[#f2c55c]/80 transition-colors group-hover:bg-[#f2c55c]"
                style={{ height: `${height}%` }}
              />
              <span className="text-text-dim mt-1 text-center font-mono text-[10px]">
                {row.nominalDay % 5 === 0 || row.nominalDay === 1 ? row.nominalDay : ''}
              </span>
            </div>
          );
        })}
      </div>
      <div className="text-text-bright mt-2 min-h-5 text-[12px]" aria-live="polite">
        {selectedDay === null
          ? 'Focus or hover a bar for its exact epoch range and amount.'
          : (() => {
              const row = evidence.windows[selectedDay - 1];
              return (
                <>
                  Nominal day {selectedDay} · epochs +{row.fromEpochOffset} through +
                  {row.throughEpochOffset} · <Amount value={row.capacity} /> CKB in{' '}
                  {row.count.toLocaleString()} deposits
                </>
              );
            })()}
      </div>
      <div className="text-text-dim mt-3 text-xs">
        {evidence.phase0.count.toLocaleString()} live deposits ·{' '}
        <Amount value={evidence.phase0.capacity} /> CKB across the full 180-epoch cycle.
      </div>
      <div className="border-base-border mt-5 grid gap-3 border-t pt-4 text-[12px] md:grid-cols-2">
        <div>
          <span className="text-[#f2c55c]">DAO epoch lock met:</span>{' '}
          <Amount value={evidence.phase1ReadyByEpochLock.capacity} /> CKB in{' '}
          {evidence.phase1ReadyByEpochLock.count.toLocaleString()} withdrawal-request cells
        </div>
        <div>
          <span className="text-text-dim">Waiting for epoch lock:</span>{' '}
          <Amount value={evidence.phase1WaitingForEpochLock.capacity} /> CKB in{' '}
          {evidence.phase1WaitingForEpochLock.count.toLocaleString()} cells
        </div>
      </div>
      <p className="text-text-dim mt-2 text-[11px]">
        Epoch-lock readiness alone does not prove a complete withdraw transaction; the required
        input, headers, witnesses and script execution still apply.
      </p>
    </Section>
  );
}

function Owners({ evidence }: { evidence: DaoEvidence }) {
  const c = evidence.concentration;
  return (
    <Section
      title="Who deposits"
      note={`all ${c.currentDepositors.toLocaleString()} current phase-0 owners`}
    >
      <div className="mb-4 grid gap-2 text-[13px] sm:grid-cols-3">
        <div>
          Largest: <strong className="text-[#f2c55c]">{c.top1SharePercent ?? '—'}%</strong>
        </div>
        <div>
          Top 10: <strong className="text-[#f2c55c]">{c.top10SharePercent ?? '—'}%</strong>
        </div>
        <div>
          Top 100: <strong className="text-[#f2c55c]">{c.top100SharePercent ?? '—'}%</strong>
        </div>
      </div>
      <div className="overflow-x-auto">
        <table className="w-full min-w-[640px] text-left text-[12px]">
          <thead className="text-text-dim border-base-border border-b">
            <tr>
              <th className="py-2">#</th>
              <th>Depositor</th>
              <th>Lock reference</th>
              <th className="whitespace-nowrap text-right">Deposits</th>
              <th className="whitespace-nowrap text-right">CKB deposited</th>
              <th className="text-right">Share</th>
            </tr>
          </thead>
          <tbody>
            {c.owners.map((owner) => (
              <tr key={owner.lockScriptHash} className="border-base-border/50 border-b">
                <td className="text-text-dim py-2">{owner.rank}</td>
                <td>
                  <Link
                    href={`/address/${owner.address}`}
                    className="text-text-bright hover:underline"
                  >
                    {owner.address.slice(0, 12)}…{owner.address.slice(-7)}
                  </Link>
                </td>
                <td
                  className="text-text-dim font-mono"
                  title={`${owner.lockCodeHash} (${owner.lockHashType})`}
                >
                  <Link
                    href={`/script/${owner.lockCodeHash}?hashType=${owner.lockHashType}&kind=lock`}
                    className="hover:text-text-bright hover:underline"
                  >
                    {owner.lockKind ?? `${owner.lockCodeHash.slice(0, 10)}…`} · {owner.lockHashType}
                  </Link>
                </td>
                <td className="whitespace-nowrap text-right font-mono">{owner.depositCount}</td>
                <td className="whitespace-nowrap text-right">
                  <Amount value={owner.capacity} />
                </td>
                <td className="text-right font-mono">{owner.sharePercent}%</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </Section>
  );
}

function RawEvidence({ evidence }: { evidence: DaoEvidence }) {
  const fields = [
    { key: 'C', bytes: '0–7', value: evidence.header.c, note: 'total issuance' },
    { key: 'AR', bytes: '8–15', value: evidence.header.ar, note: 'accumulated rate × 10¹⁶' },
    {
      key: 'S',
      bytes: '16–23',
      value: evidence.header.s,
      note: 'compensation still owed to live deposits + the burnt share',
    },
    { key: 'U', bytes: '24–31', value: evidence.header.u, note: 'protocol occupied capacity' },
  ];
  return (
    <DetailTier2>
      <DetailSection
        title={`The DAO field in block ${formatBlockNumber(evidence.header.blockNumber)}`}
        actions={<Note>32 raw header bytes · four little-endian u64 fields</Note>}
      >
        <div className="grid gap-2 md:grid-cols-4">
          {fields.map((field) => (
            <div key={field.key} className="border-base-border border-l pl-3">
              <div className="font-mono text-[15px] text-[#f2c55c]">
                {field.key} <span className="text-text-dim text-[11px]">bytes {field.bytes}</span>
              </div>
              <div className="text-text-bright break-all font-mono text-[12px]">
                {field.value}{' '}
                <span className="text-text-dim">
                  {field.key === 'AR' ? 'scaled rate' : 'shannons'}
                </span>
              </div>
              {field.key !== 'AR' && (
                <div className="text-text-bright text-[12px]">
                  <Amount value={field.value} /> CKB
                </div>
              )}
              <div className="text-text-dim text-[11px]">{field.note}</div>
            </div>
          ))}
        </div>
        <div className="text-text-dim mt-4 break-all font-mono text-[11px]">
          {evidence.header.daoHex}
        </div>
        <Link
          href={`/blocks/${evidence.header.blockNumber}`}
          className="mt-2 inline-block text-[12px] text-[#f2c55c] hover:underline"
        >
          Open source block →
        </Link>
      </DetailSection>
      <DetailSection title="The DAO script" actions={<Note>type script · genesis deployment</Note>}>
        <Link
          href="/script/0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e?hashType=type&kind=type"
          className="text-text-bright break-all font-mono text-[12px] hover:text-[#f2c55c] hover:underline"
        >
          0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e
        </Link>
        <p className="text-text-dim mt-2 text-[12px]">
          Eight zero data bytes mark a deposit. A withdrawal request stores its deposit block number
          as little-endian u64. A completed withdrawal cites the deposit and request headers and an
          absolute epoch <code>since</code> at its required cycle boundary.
        </p>
      </DetailSection>
    </DetailTier2>
  );
}

export function DaoBoard({
  statistics,
  evidence,
  depositedHistory,
  circulationHistory,
  historyError,
}: Props) {
  if (
    evidence.phase0.capacity !== statistics.totalDeposited &&
    evidence.tipBlockNumber === statistics.tipBlockNumber
  ) {
    throw new Error(
      `DAO phase-0 capacity disagrees with indexed statistics at block ${evidence.tipBlockNumber}`
    );
  }
  return (
    <div>
      <div className="space-y-8">
        <Hero
          statistics={statistics}
          evidence={evidence}
          deposited={depositedHistory}
          ratio={circulationHistory}
        />
        <History deposited={depositedHistory} ratio={circulationHistory} error={historyError} />
        <Issuance statistics={statistics} />
        <Section title="How a deposit grows" note="header AR · free capacity only">
          <p className="text-text-dim text-[13px]">
            AR grew from {formatAr(evidence.ar.genesisAr)} at genesis to{' '}
            {formatAr(evidence.ar.tipAr)} at block {formatBlockNumber(evidence.tipBlockNumber)}: +
            {evidence.ar.sinceGenesisGrowthPercent}% on a deposit’s free capacity since genesis.
            Compensation freezes at the withdrawal request AR. Estimated APC above models current
            issuance; the observed growth uses the last canonical block at or before the 365-day
            chain-time cutoff.
          </p>
          <Chart
            title="Accumulated rate"
            subtitle="Canonical yearly chain-time anchors and indexed tip"
            data={evidence.ar.milestones.map((row) => ({
              label: `${formatBlockNumber(row.blockNumber)}`,
              values: { ar: row.arIndex },
            }))}
            series={[{ key: 'ar', label: 'AR', color: '#f2c55c' }]}
            unit="×"
            kind="line"
            height={165}
            formatValue={(value) => formatExactDecimal(value, 'DAO accumulated rate')}
          />
          {evidence.ar.year365 && (
            <div className="text-text-dim mt-2 font-mono text-[11px]">
              365-day anchor: block {formatBlockNumber(evidence.ar.year365.anchorBlockNumber)} ·
              chain time {formatUtc8DateTime(evidence.ar.year365.anchorChainTimeMs)} · AR{' '}
              {formatAr(evidence.ar.year365.anchorAr)} · observed span{' '}
              {formatDurationMs(
                exactSafeMs(evidence.ar.year365.elapsedChainTimeMs, 'DAO 365-day observed span')
              )}
            </div>
          )}
        </Section>
        <Windows evidence={evidence} />
        <Owners evidence={evidence} />
        <DaoSources />
      </div>
      <RawEvidence evidence={evidence} />
    </div>
  );
}
