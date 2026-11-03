'use client';

import { useState } from 'react';
import type { ReactNode } from 'react';
import Link from '@/components/ui/link';
import type { ChartResponse, DaoEvidence, DaoStatistics } from '@/lib/api';
import { formatPercent, parseCkbDecimal, parseShannons, splitCkbAmount } from '@/lib/ckb-decimal';
import { DaoSources } from '@/components/dao/dao-sources';
import {
  DaoHistoryFigure,
  DaoHistoryReading,
  HistoryRangeToggle,
  historySeries,
  type HistoryRange,
} from '@/components/dao/dao-history';
import { DaoGrows } from '@/components/dao/dao-grows';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { formatAccumulatedRate } from '@/lib/dao-header';
import { exactUnsignedInteger } from '@/lib/exact-integer';
import { formatCapacity, formatDurationMs, formatUtc8DateTime, truncateHash } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';
import { BlockLink } from '@/components/ui/block-link';
import { IciclePartition, type IcicleNode } from '@/components/ui/icicle-partition';
import { useElementWidth } from '@/hooks/useElementWidth';
import { DAO_ISSUANCE_COLORS } from '@/lib/design-tokens';
import { ConcentrationBar } from '@/components/ui/concentration-bar';
import { ListTable, type ListColumn } from '@/components/ui/list-table';
import { DetailSection, DetailTier2 } from '@/components/ui/detail-layout';
import { ReadoutRow } from '@/components/ui/readout-row';

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

/** Header AR integers (×10¹⁶) as the exact rate, the same form the block page decodes. */
function formatAr(raw: string): string {
  if (!/^\d+$/.test(raw)) throw new Error(`DAO AR "${raw}" is not an unsigned integer`);
  return formatAccumulatedRate(BigInt(raw));
}

function Section({
  title,
  note,
  children,
}: {
  title: string;
  /** Only "Who deposits" keeps a note beside its heading (its link to every depositor). */
  note?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="min-w-0" aria-label={title}>
      <div className="mb-[18px] flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h2 className="text-text-bright text-section font-medium">{title}</h2>
        {note && <span className="text-text-dim text-meta">{note}</span>}
      </div>
      {children}
    </section>
  );
}

/** One metric of the hero's row (R13): a meta label over a section-size value and its inline note. */
function Metric({
  label,
  children,
  note,
}: {
  label: string;
  children: ReactNode;
  note?: ReactNode;
}) {
  return (
    <div className="min-w-0">
      <dt className="text-text-dim text-meta">{label}</dt>
      <dd className="text-text-bright text-section mt-0.5 tabular-nums">
        {children}
        {note && <span className="text-text-dim text-small"> · {note}</span>}
      </dd>
    </div>
  );
}

/**
 * The page's one figure (R4, R11): display from 640 up, index below, in the DAO gold — the only
 * gold text in tier 1; the fraction hero in the dim gold, the unit body.
 */
function DepositedFigure({ shannons }: { shannons: string }) {
  const parts = splitCkbAmount(parseShannons(shannons, 'DAO deposited'));
  return (
    <div data-role="deposited" className="text-gold text-index sm:text-display font-medium">
      <span className="whitespace-nowrap font-mono tabular-nums">
        {parts.integer}
        <span className="text-hero text-gold-dim font-normal">
          {parts.fraction}
          <span className="opacity-50">{parts.ghost}</span>
        </span>
      </span>
      {/* The space takes the unit's size, so " CKB" ends inside a 358 px column at 390. */}
      <span className="text-body text-text-dim font-normal"> CKB</span>
    </div>
  );
}

/** A signed CKB decimal in its sign's hue: the fraction in the same hue's dim step. */
function SignedCkbDecimal({ value, fractionClass }: { value: string; fractionClass: string }) {
  const parts = splitCkbAmount(parseCkbDecimal(value));
  return (
    <span className="whitespace-nowrap font-mono tabular-nums">
      {parts.sign}
      {parts.integer}
      <span className={fractionClass}>{parts.fraction}</span>
      <span className="opacity-40">{parts.ghost}</span>
    </span>
  );
}

/** A signed whole count: "+9", "−9" (U+2212), "0". */
function signedCount(n: number): string {
  const abs = Math.abs(n).toLocaleString('en-US');
  return n > 0 ? `+${abs}` : n < 0 ? `−${abs}` : abs;
}

/** The page title (index step), in the hero's title row and on the page while it loads. */
export function DaoHeading() {
  return <h1 className="text-text-bright text-index font-medium">Nervos DAO</h1>;
}

/** R13: one short sentence (≤ 90 characters) and the way down to the explanation in tier 2. */
export function DaoLede() {
  return (
    <p className="text-text-dim text-body mt-1.5 max-w-[560px]">
      CKB locked in the DAO keeps its share of the supply.{' '}
      <a href="#evidence" className="hover:text-text-bright whitespace-nowrap hover:underline">
        How it works ↓
      </a>
    </p>
  );
}

/**
 * The hero (hierarchy board v3, R13): the title row (h1, the DAO charts link, the range buttons),
 * one sentence, then deposited as the page's one gold figure with its 24 h change on its baseline
 * row, and the picture right under it. The picture's reading row and the four metrics in one row
 * follow the hero section. Deposited, deposits and depositors are the evidence anchor's phase-0
 * facts; Estimated APC and the 24 h figures are the statistics anchor's.
 */
function Hero({
  statistics,
  evidence,
  deposited,
  ratio,
  historyError,
}: {
  statistics: DaoStatistics;
  evidence: DaoEvidence;
  deposited?: DaoDepositHistory;
  ratio?: ChartResponse;
  historyError?: boolean;
}) {
  const [range, setRange] = useState<HistoryRange>('All');
  const pin = usePinnedReadout<number>();
  if (deposited && deposited.peak === undefined)
    throw new Error('/dao/charts/total-deposit has no peak (R8 chart contract)');
  if ((statistics.deposits24h === null) !== (statistics.withdrawalRequests24h === null))
    throw new Error(
      `DAO statistics at ${formatBlockNumber(statistics.tipBlockNumber)}: 24 h deposits ${statistics.deposits24h} without its withdrawal-request count`
    );
  const series =
    !historyError && deposited && ratio ? historySeries(deposited, ratio, range) : null;
  const change = statistics.depositChange24h;
  const changeShannons = change === null ? null : parseCkbDecimal(change);
  const year = evidence.ar.year365;
  return (
    <div className="min-w-0">
      <section aria-label="Nervos DAO" className="min-w-0">
        <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-2">
          <DaoHeading />
          <div className="flex flex-wrap items-center gap-x-5 gap-y-2">
            {/* The hero's former "Explore DAO charts →", beside the range of the charts it continues. */}
            <Link
              href="/charts/total-deposit"
              className="text-text hover:text-text-bright text-small font-mono hover:underline"
            >
              Explore DAO charts →
            </Link>
            {series && (
              <HistoryRangeToggle
                range={range}
                onChange={(next) => {
                  setRange(next);
                  pin.clear();
                }}
              />
            )}
          </div>
        </div>
        <DaoLede />
        <div data-role="figure" className="mt-9 min-w-0">
          <div className="text-text-dim text-meta mb-1.5 uppercase tracking-[0.08em]">
            deposited
          </div>
          <div className="flex flex-wrap items-baseline justify-between gap-x-6 gap-y-2">
            <DepositedFigure shannons={evidence.phase0.capacity} />
            <p className="text-body">
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
                  <SignedCkbDecimal
                    value={change!}
                    fractionClass={
                      changeShannons > BigInt(0)
                        ? 'text-positive-dim'
                        : changeShannons < BigInt(0)
                          ? 'text-negative-dim'
                          : 'text-text-dim'
                    }
                  />{' '}
                  CKB
                </span>
              )}{' '}
              <span className="text-text-dim">
                in 24 h
                {statistics.deposits24h !== null &&
                  statistics.withdrawalRequests24h !== null &&
                  ` · ${plural(statistics.deposits24h, 'deposit')} in, ${plural(statistics.withdrawalRequests24h, 'withdrawal request')} out`}
              </span>
            </p>
          </div>
        </div>
        {historyError ? (
          <div role="alert" className="text-rouge text-small mt-[18px]">
            DAO daily history could not be loaded from indexed snapshots.
          </div>
        ) : series === null ? (
          <div className="text-text-dim text-small mt-[18px]">Loading canonical daily history…</div>
        ) : (
          <DaoHistoryFigure series={series} pin={pin} />
        )}
      </section>
      {series && (
        <div className="mt-2.5">
          <DaoHistoryReading series={series} pin={pin} peak={deposited?.peak ?? null} />
        </div>
      )}
      <dl className="mt-7 grid grid-cols-2 gap-x-8 gap-y-4 lg:grid-cols-4">
        <Metric label="Estimated APC">{statistics.estimatedApc}%</Metric>
        <Metric
          label="Compensation, last 365 days"
          note={year ? undefined : 'the chain is younger than 365 days'}
        >
          {year ? `${year.observedGrowthPercent}%` : '—'}
        </Metric>
        <Metric
          label="Depositors"
          note={
            statistics.depositorsChange24h === null
              ? undefined
              : `${signedCount(statistics.depositorsChange24h)} in 24 h`
          }
        >
          {evidence.concentration.currentDepositors.toLocaleString('en-US')}
        </Metric>
        <Metric
          label="Deposits"
          note={`${evidence.phase1.count.toLocaleString('en-US')} withdrawals requested`}
        >
          {evidence.phase0.count.toLocaleString('en-US')}
        </Metric>
      </dl>
    </div>
  );
}

/** An exact millisecond string from the API as a safe integer for duration display. */
function exactSafeMs(value: string, context: string): number {
  const ms = exactUnsignedInteger(value, context);
  if (ms > BigInt(Number.MAX_SAFE_INTEGER))
    throw new Error(`${context}: ${value} ms is out of range`);
  return Number(ms);
}

/** An exact CKB amount inside SVG text: whole part, then the significant decimals dimmed. */
function SvgCkb({ shannons }: { shannons: bigint }) {
  const parts = splitCkbAmount(shannons);
  return (
    <>
      {parts.integer}
      <tspan fillOpacity={0.7}>{parts.fraction}</tspan>
    </>
  );
}

const ICICLE_LABEL_MIN_PX = 1000;
/** Hierarchy v3 (board): the icicle's rows — the whole, its three parts, compensation's two. */
const ICICLE_ROWS = { heights: [48, 48, 32], gap: 3 } as const;
/**
 * A monospace advance in px: Chromium snaps JetBrains Mono to whole pixels (11 and 12 px → 7,
 * 13 px → 8); a browser that does not uses 0.6 em. Fit tests use whichever is wider.
 */
const monoAdvance = (px: number) => Math.max(Math.round(0.6 * px), 0.6 * px);
/** The capacity-use bar's segments sit 3 px apart. */
const BAR_GAP_PX = 3;

/** The parts of secondary issuance since genesis, exact, checked to partition the whole. */
function issuanceParts(statistics: DaoStatistics) {
  const mining = parseShannons(statistics.miningReward, 'miningReward');
  const compensation = parseShannons(statistics.depositCompensation, 'depositCompensation');
  const burnt = parseShannons(statistics.burnt, 'burnt');
  const claimed = parseShannons(statistics.totalCompensationPaid, 'totalCompensationPaid');
  const unclaimed = parseShannons(statistics.unclaimedCompensation, 'unclaimedCompensation');
  if (claimed + unclaimed !== compensation)
    throw new Error('DAO compensation parts do not equal total compensation');
  const liveCells = (
    statistics.depositCellsCount + statistics.withdrawalRequestCellsCount
  ).toLocaleString('en-US');
  return {
    mining,
    compensation,
    burnt,
    claimed,
    unclaimed,
    total: mining + compensation + burnt,
    liveCells,
  };
}

/**
 * "Where secondary issuance goes" (hierarchy v3, R12): the icicle names each part and its share
 * inside its box; every exact amount is one quiet line under it (a list at phone width, where the
 * boxes are too narrow for words); one caption. How every block splits its issuance — capacity
 * use — and the protocol notes are tier 2 ("How the Nervos DAO works").
 */
function Issuance({ statistics }: Pick<Props, 'statistics'>) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
  const { mining, compensation, burnt, claimed, unclaimed, total, liveCells } =
    issuanceParts(statistics);
  const issuanceEmpty = total === BigInt(0);
  const share = (v: bigint) => `${formatPercent(v, total, 2)}%`;
  const ofCompensation = (v: bigint) => `${formatPercent(v, compensation, 1)}%`;
  const root: IcicleNode | null = issuanceEmpty
    ? null
    : {
        key: 'total',
        label: 'Secondary issuance since genesis',
        value: total,
        tone: 'ckb',
        color: DAO_ISSUANCE_COLORS.total,
        opacity: 0.55,
        ink: 'bright',
        // The board's root label: the whole as a whole number of CKB (cut, not rounded); the
        // exact parts are the line under the icicle.
        boxLabel: {
          name: 'secondary issuance since genesis',
          share: splitCkbAmount(total).integer,
          unit: 'CKB',
        },
        children: [
          {
            key: 'miners',
            label: 'to miners',
            value: mining,
            tone: 'ckb',
            color: DAO_ISSUANCE_COLORS.miners,
            opacity: 1,
            ink: 'dark',
            boxLabel: { name: 'miners', share: share(mining) },
          },
          {
            key: 'compensation',
            label: 'compensation for depositors',
            value: compensation,
            tone: 'dao',
            opacity: 1,
            ink: 'dark',
            boxLabel: { name: 'depositors', share: share(compensation) },
            children: [
              {
                key: 'paid',
                label: 'paid out',
                value: claimed,
                tone: 'dao',
                opacity: 1,
                ink: 'dark',
                boxLabel: { name: 'paid out', share: ofCompensation(claimed) },
              },
              {
                key: 'owed',
                label: 'still owed',
                value: unclaimed,
                tone: 'dao',
                opacity: 0.4,
                ink: 'bright',
                boxLabel: { name: 'still owed', share: ofCompensation(unclaimed) },
              },
            ],
          },
          {
            key: 'burnt',
            label: 'burnt',
            value: burnt,
            tone: 'other',
            color: DAO_ISSUANCE_COLORS.burnt,
            opacity: 1,
            ink: 'bright',
            boxLabel: { name: 'burnt', share: share(burnt), note: 'no cell can ever claim it' },
          },
        ],
      };
  const narrow = width !== null && width < ICICLE_LABEL_MIN_PX;
  return (
    <Section title="Where secondary issuance goes">
      <div ref={ref} className="min-w-0">
        {root === null ? (
          <div className="text-text-dim text-small">
            No secondary issuance has accrued at this indexed block; allocation shares are
            undefined.
          </div>
        ) : (
          width !== null && (
            <IciclePartition
              root={root}
              label="Secondary issuance since genesis, split three ways: to miners, compensation for depositors, burnt; the compensation is paid out or still owed to live deposits"
              formatValue={(v) => <SvgCkb shannons={v} />}
              width={width}
              rows={ICICLE_ROWS}
              hideLabels={narrow}
            />
          )
        )}
      </div>
      {root !== null && width !== null && narrow && (
        <ul
          aria-label="Secondary issuance parts"
          className="text-small text-text-dim mt-3 space-y-1.5"
        >
          <li>
            secondary issuance since genesis{' '}
            <span className="text-text">
              <Amount value={total.toString()} />
            </span>{' '}
            CKB
          </li>
          <li>
            <Swatch color={DAO_ISSUANCE_COLORS.miners} />
            miners {share(mining)} ·{' '}
            <span className="text-text">
              <Amount value={mining.toString()} />
            </span>
          </li>
          <li>
            <Swatch color={DAO_ISSUANCE_COLORS.compensation} />
            depositors {share(compensation)} ·{' '}
            <span className="text-text">
              <Amount value={compensation.toString()} />
            </span>
            <ul className="mt-1 space-y-1 pl-5">
              <li>
                paid out {ofCompensation(claimed)} ·{' '}
                <span className="text-text">
                  <Amount value={claimed.toString()} />
                </span>
              </li>
              <li>
                still owed {ofCompensation(unclaimed)} ·{' '}
                <span className="text-text">
                  <Amount value={unclaimed.toString()} />
                </span>{' '}
                to {liveCells} live cells
              </li>
            </ul>
          </li>
          <li>
            <Swatch color={DAO_ISSUANCE_COLORS.burnt} />
            burnt {share(burnt)} ·{' '}
            <span className="text-text">
              <Amount value={burnt.toString()} />
            </span>{' '}
            · no cell can ever claim it
          </li>
        </ul>
      )}
      {root !== null && width !== null && !narrow && (
        // One line of three facts (each its own item, so a narrower column wraps between them).
        <p
          data-role="exact-amounts"
          className="text-small text-text-dim mt-3 flex flex-wrap gap-x-[0.6em]"
        >
          <span>
            to miners{' '}
            <span className="text-text">
              <Amount value={mining.toString()} />
            </span>
          </span>
          <span>
            {' '}
            · for depositors{' '}
            <span className="text-text">
              <Amount value={compensation.toString()} />
            </span>{' '}
            (paid out <Amount value={claimed.toString()} /> · still owed{' '}
            <Amount value={unclaimed.toString()} /> to {liveCells} live cells)
          </span>
          <span>
            {' '}
            · burnt{' '}
            <span className="text-text">
              <Amount value={burnt.toString()} />
            </span>{' '}
            CKB
          </span>
        </p>
      )}
      <p className="text-text text-body mt-3.5 max-w-[640px]">
        1.344 billion CKB a year, split in every block by how CKB is used: a deposit keeps its
        share, free CKB outside the DAO has its share burnt.
      </p>
    </Section>
  );
}

/** A mechanism tile (the "Two names for the same script" chain's tile, `script-byte-map.tsx`). */
function MechanismTile({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="text-small text-text-dim min-w-0 flex-1 rounded-md border border-[#1a1f30] px-3 py-2.5">
      <b className="text-text-bright mb-0.5 block font-medium">{title}</b>
      {children}
    </div>
  );
}

function MechanismArrow() {
  return (
    <span aria-hidden="true" className="text-center text-[#343c50] sm:w-5 sm:self-center">
      <span className="sm:hidden">↓</span>
      <span className="hidden sm:inline">→</span>
    </span>
  );
}

/**
 * Material pass v2 (7): the first mechanism, drawn. Each block's secondary issuance s fans out
 * three ways, by how all CKB is used at that block; the words are the explanation's own.
 */
function IssuanceSplit() {
  return (
    <div
      role="group"
      aria-label="How each block's secondary issuance is split"
      className="mt-3.5 flex flex-col gap-1.5 sm:flex-row sm:items-stretch"
    >
      <MechanismTile title="secondary issuance s">
        of each block: 1.344 billion CKB a year, split by how all CKB issued is used at that block
      </MechanismTile>
      <MechanismArrow />
      <ul className="flex min-w-0 flex-col gap-1.5 sm:flex-[2]">
        <li className="flex">
          <MechanismTile title="miners">
            the occupied share, s × U ÷ C, paid in the cellbase
          </MechanismTile>
        </li>
        <li className="flex">
          <MechanismTile title="depositors">
            the deposits&apos; share, compensation accrued via AR, paid out at withdrawal
          </MechanismTile>
        </li>
        <li className="flex">
          <MechanismTile title="burnt">everything else; no cell can ever claim it</MechanismTile>
        </li>
      </ul>
    </div>
  );
}

/**
 * Material pass v2 (7): the second mechanism, drawn. A deposit's life in three cells, with where
 * its compensation stops and when it can leave; the words are the explanation's own.
 */
function DepositLife() {
  return (
    <ol
      aria-label="A deposit's life"
      className="mt-3.5 flex flex-col gap-1.5 sm:flex-row sm:items-stretch"
    >
      <li className="flex min-w-0 sm:flex-1">
        <MechanismTile title="deposit">
          a DAO cell; its 180-epoch cycle is counted from the epoch it was made
        </MechanismTile>
      </li>
      <li className="flex min-w-0 flex-col gap-1.5 sm:flex-1 sm:flex-row">
        <MechanismArrow />
        <MechanismTile title="withdrawal request">
          spends the deposit; its compensation stops growing here, at this AR
        </MechanismTile>
      </li>
      <li className="flex min-w-0 flex-col gap-1.5 sm:flex-1 sm:flex-row">
        <MechanismArrow />
        <MechanismTile title="withdrawal">
          at the end of the current cycle, about 30 days: the deposit and its compensation leave the
          DAO
        </MechanismTile>
      </li>
    </ol>
  );
}

/**
 * Capacity use at the statistics block (tier 2 since hierarchy v3): how all CKB issued so far is
 * used, the proportions every block splits its secondary issuance by — an 18 px bar with each share
 * inside the segments that fit (R9), a legend, and the line naming the block.
 */
function CapacityUse({ statistics }: { statistics: DaoStatistics }) {
  const [ref, width] = useElementWidth<HTMLDivElement>();
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
  const use = [
    { key: 'occupied', title: 'Occupied', value: occupied, color: DAO_ISSUANCE_COLORS.miners },
    {
      key: 'depositors',
      title: 'DAO depositors',
      value: depositorWeight,
      color: DAO_ISSUANCE_COLORS.compensation,
    },
    { key: 'other', title: 'Everything else', value: other, color: DAO_ISSUANCE_COLORS.burnt },
  ];
  return (
    <div ref={ref} className="mt-3.5 flex min-w-0 flex-col">
      <div className="text-text-dim text-meta">
        How all{' '}
        <span className="text-text">
          <Amount value={issued.toString()} />
        </span>{' '}
        CKB issued so far is used at block <BlockLink number={statistics.tipBlockNumber} /> · every
        block splits its issuance in these proportions
      </div>
      <div
        className="text-meta mt-1.5 flex h-[18px] w-full gap-[3px] overflow-hidden"
        aria-label="Current capacity use partition"
      >
        {use.map((part) => {
          const percent = formatPercent(part.value, issued, 4);
          const label =
            part.key === 'other'
              ? `everything else ${formatPercent(part.value, issued, 2)}% → burnt`
              : `${formatPercent(part.value, issued, 2)}%`;
          // R9: a share is written inside its segment only where it fits (padding included).
          const segmentPx =
            width === null ? 0 : ((width - BAR_GAP_PX * (use.length - 1)) * Number(percent)) / 100;
          const fits = segmentPx >= label.length * monoAdvance(11) + 16;
          return (
            <div
              key={part.key}
              title={`${part.title}: ${formatCapacity(part.value)}`}
              className={`flex min-w-0 items-center overflow-hidden whitespace-nowrap rounded-[3px] ${part.key === 'other' ? 'text-text flex-1 px-2.5' : 'text-base-bg shrink-0 justify-center'}`}
              style={{
                width: part.key === 'other' ? undefined : `${percent}%`,
                background: part.color,
              }}
            >
              {fits ? label : ''}
            </div>
          );
        })}
      </div>
      <div className="text-text-dim text-meta mt-1.5 flex flex-wrap gap-x-6 gap-y-1">
        <span>
          <Swatch color={DAO_ISSUANCE_COLORS.miners} />
          occupied by cells <span className="text-text">
            {formatPercent(occupied, issued, 2)}%
          </span>{' '}
          → miners
        </span>
        <span>
          <Swatch color={DAO_ISSUANCE_COLORS.compensation} />
          DAO deposits, free capacity + compensation accrued{' '}
          <span className="text-text">{formatPercent(depositorWeight, issued, 2)}%</span> →
          depositors
        </span>
        <span>
          <Swatch color={DAO_ISSUANCE_COLORS.burnt} />
          everything else <span className="text-text">{formatPercent(other, issued, 2)}%</span> →
          burnt
        </span>
      </div>
    </div>
  );
}

function Swatch({ color }: { color: string }) {
  return (
    <span
      aria-hidden="true"
      className="mr-1.5 inline-block h-[9px] w-[9px] rounded-sm align-middle"
      style={{ background: color }}
    />
  );
}

function plural(count: number, word: string): string {
  return `${count.toLocaleString('en-US')} ${word}${count === 1 ? '' : 's'}`;
}

/**
 * "When deposits can leave" (hierarchy v3, V4): 200 px bars of the CKB whose cycle ends on each of
 * the next 30 days, named inside the plot, one axis line and one reading row; the requested
 * withdrawals as one 28 px bar with the claimable amount inside it; one caption. How to read the
 * bars, the indexed epoch and the epoch-lock caveat are tier 2.
 */
function Windows({ evidence }: { evidence: DaoEvidence }) {
  const [selectedDay, setSelectedDay] = useState<number | null>(null);
  const [barRef, barWidth] = useElementWidth<HTMLDivElement>();
  const max = evidence.windows.reduce((largest, row) => {
    const value = parseShannons(row.capacity, 'window capacity');
    return value > largest ? value : largest;
  }, BigInt(0));
  const first = evidence.windows[0];
  if (!first || first.nominalDay !== 1)
    throw new Error(
      `DAO evidence at ${formatBlockNumber(evidence.tipBlockNumber)} has no first-day cycle window`
    );
  const ready = evidence.phase1ReadyByEpochLock;
  const waiting = evidence.phase1WaitingForEpochLock;
  const readyShannons = parseShannons(ready.capacity, 'phase-1 ready capacity');
  const waitingShannons = parseShannons(waiting.capacity, 'phase-1 waiting capacity');
  const requested = parseShannons(evidence.phase1.capacity, 'phase-1 capacity');
  if (
    readyShannons + waitingShannons !== requested ||
    ready.count + waiting.count !== evidence.phase1.count
  )
    throw new Error(
      `DAO phase-1 readiness does not partition the requests at ${formatBlockNumber(evidence.tipBlockNumber)}`
    );
  const claimable = (
    <>
      claimable now{' '}
      <span className="text-text-bright">
        <Amount value={ready.capacity} /> CKB
      </span>{' '}
      <span className="text-text-dim">· {plural(ready.count, 'cell')}</span>
    </>
  );
  // R12: the claimable words sit inside their segment where they fit (12 px, 12 px padding).
  const readyPercent = requested > BigInt(0) ? formatPercent(readyShannons, requested, 4) : '0';
  const claimableChars =
    `claimable now ${formatCapacity(readyShannons)} · ${plural(ready.count, 'cell')}`.length;
  const claimableInside =
    barWidth !== null &&
    ((barWidth - 2) * Number(readyPercent)) / 100 >= claimableChars * monoAdvance(12) + 24;
  return (
    <Section title="When deposits can leave">
      <div className="relative">
        <span className="text-text-dim text-meta pointer-events-none absolute left-2 top-1">
          CKB whose 180-epoch cycle ends in the next 30 days
        </span>
        <div
          className="grid grid-cols-[repeat(30,minmax(0,1fr))] gap-[3px]"
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
                className="focus:outline-interactive group flex h-[200px] flex-col justify-end pt-6 focus:outline"
                title={`Nominal day ${row.nominalDay}, epochs +${row.fromEpochOffset} to +${row.throughEpochOffset}: ${row.count} deposits, ${row.capacity} shannons`}
              >
                <div
                  data-window-bar
                  className={`bg-gold w-full rounded-t-sm transition-opacity group-hover:opacity-100 ${row.nominalDay === 1 ? 'opacity-90' : 'opacity-50'}`}
                  style={{ height: `${height}%` }}
                />
              </div>
            );
          })}
        </div>
      </div>
      <div aria-hidden="true" className="text-text-dim text-meta mt-1 flex justify-between">
        <span>in a day</span>
        <span>day 10</span>
        <span>day 20</span>
        <span>day 30</span>
      </div>
      {/* Hierarchy D5 / v3: one readout row under the bars, opening on the next day's cycle end. */}
      <div className="mt-2.5">
        <ReadoutRow
          label="Cycle reading"
          pinned={false}
          idle={
            <>
              <span className="text-text-bright">
                <Amount value={first.capacity} /> CKB
              </span>{' '}
              in {plural(first.count, 'deposit')} ends its cycle within a day
            </>
          }
        >
          {selectedDay === null
            ? null
            : (() => {
                const row = evidence.windows[selectedDay - 1];
                return (
                  <span className="text-text-bright">
                    Nominal day {selectedDay} · epochs +{row.fromEpochOffset} through +
                    {row.throughEpochOffset} · <Amount value={row.capacity} /> CKB in{' '}
                    {row.count.toLocaleString()} deposits
                  </span>
                );
              })()}
        </ReadoutRow>
      </div>
      <div className="mt-5 flex flex-col">
        <div className="text-text-dim text-meta">
          withdrawals already requested · {plural(evidence.phase1.count, 'cell')}
        </div>
        {requested > BigInt(0) && (
          <div ref={barRef} className="mt-1.5 min-w-0">
            <div
              role="img"
              aria-label={`Withdrawal requests: past the window ${formatCapacity(readyShannons)}, waiting ${formatCapacity(waitingShannons)}`}
              className="text-small flex h-[28px] w-full gap-0.5 overflow-hidden"
            >
              <span
                className="bg-gold/45 text-text flex items-center overflow-hidden whitespace-nowrap rounded-[3px] px-3"
                style={{ width: `${readyPercent}%` }}
              >
                {/* One flex item, so the spaces between its words stay. */}
                {claimableInside ? <span>{claimable}</span> : null}
              </span>
              <span
                className="bg-gold rounded-[3px]"
                style={{ width: `${formatPercent(waitingShannons, requested, 4)}%` }}
              />
            </div>
            <div
              className={`text-text-dim text-meta mt-1 flex flex-wrap gap-x-6 gap-y-0.5 ${claimableInside ? 'justify-end text-right' : 'justify-between'}`}
            >
              {!claimableInside && <span>{claimable}</span>}
              <span>
                waiting for its window{' '}
                <span className="text-text">
                  <Amount value={waiting.capacity} /> CKB
                </span>{' '}
                · {plural(waiting.count, 'cell')}
              </span>
            </div>
          </div>
        )}
      </div>
      <p className="text-text text-body mt-3.5 max-w-[640px]">
        A withdrawal completes only at the end of a 180-epoch cycle, about 30 days.
      </p>
    </Section>
  );
}

type Owner = DaoEvidence['concentration']['owners'][number];

/** Hierarchy R8: rows of a list on a topic page; the list page behind its link has them all. */
const TOPIC_ROWS = 5;

/**
 * The ranked groups of the concentration bar from the evidence's exact top-1 / 10 / 100 sums:
 * largest · next 9 · next 90 · the rest, naming only groups that have members.
 */
function concentrationGroups(evidence: DaoEvidence) {
  const c = evidence.concentration;
  const n = c.currentDepositors;
  const total = parseShannons(evidence.phase0.capacity, 'phase-0 capacity');
  const top = [
    parseShannons(c.top1Capacity, 'top1Capacity'),
    parseShannons(c.top10Capacity, 'top10Capacity'),
    parseShannons(c.top100Capacity, 'top100Capacity'),
    total,
  ];
  const bounds = [1, 10, 100, n];
  const groups: { label: string; value: bigint }[] = [];
  let before = 0;
  let previous = BigInt(0);
  bounds.forEach((bound, i) => {
    const members = Math.min(bound, n) - before;
    const value = top[i] - previous;
    if (members > 0) {
      const label =
        i === 0
          ? 'largest'
          : i === 3
            ? `the other ${members.toLocaleString('en-US')}`
            : `next ${members.toLocaleString('en-US')}`;
      groups.push({ label, value });
    } else if (value !== BigInt(0)) {
      throw new Error(
        `DAO concentration at ${formatBlockNumber(evidence.tipBlockNumber)}: ${value} shannons in an empty rank group`
      );
    }
    before = Math.max(before, Math.min(bound, n));
    previous = top[i];
  });
  return { total, groups };
}

function ownerLink(owner: Owner) {
  return (
    <Link href={`/address/${owner.address}`} className="text-text-bright hover:underline">
      {owner.address.slice(0, 12)}…{owner.address.slice(-7)}
    </Link>
  );
}

function lockLink(owner: Owner) {
  return (
    <Link
      href={`/script/${owner.lockCodeHash}?hashType=${owner.lockHashType}&kind=lock`}
      className="text-text-dim hover:text-text-bright font-mono hover:underline"
      title={`${owner.lockCodeHash} (${owner.lockHashType})`}
    >
      {owner.lockKind ?? `${owner.lockCodeHash.slice(0, 10)}…`}
    </Link>
  );
}

/**
 * Board v2: depositor rows read at body (13 px) under 12 px column heads, as the board sets them;
 * v3 (R15): 6 px above and below the line, rows of 32–34 px.
 */
const OWNER_CELL = 'text-body py-1.5';

/** Board v3: the column heads at meta (11 px), under which the rows read at body. */
const ownerHead = (label: string) => <span className="text-meta">{label}</span>;

const OWNER_COLUMNS: ListColumn<Owner>[] = [
  {
    key: 'rank',
    className: OWNER_CELL,
    label: '#',
    header: ownerHead('#'),
    minWidth: 40,
    render: (o) => <span className="text-text-dim">{o.rank}</span>,
  },
  {
    key: 'depositor',
    label: 'Depositor',
    header: ownerHead('Depositor'),
    minWidth: 190,
    className: OWNER_CELL,
    render: ownerLink,
  },
  {
    key: 'lock',
    label: 'Lock',
    header: ownerHead('Lock'),
    minWidth: 170,
    className: OWNER_CELL,
    render: lockLink,
  },
  {
    key: 'deposits',
    className: OWNER_CELL,
    label: 'Deposits',
    header: ownerHead('Deposits'),
    minWidth: 80,
    numeric: true,
    render: (o) => o.depositCount.toLocaleString('en-US'),
  },
  {
    key: 'capacity',
    className: OWNER_CELL,
    label: 'CKB deposited',
    header: ownerHead('CKB deposited'),
    minWidth: 210,
    numeric: true,
    render: (o) => <Amount value={o.capacity} />,
  },
  {
    key: 'share',
    className: OWNER_CELL,
    label: 'Share',
    header: ownerHead('Share'),
    minWidth: 70,
    numeric: true,
    render: (o) => <span className="text-text-dim">{o.sharePercent}%</span>,
  },
];

function Owners({ evidence }: { evidence: DaoEvidence }) {
  const { total, groups } = concentrationGroups(evidence);
  return (
    <Section
      title="Who deposits"
      note={
        <Link
          href="/dao/depositors"
          className="text-text hover:text-text-bright text-small hover:underline"
        >
          All {evidence.concentration.currentDepositors.toLocaleString('en-US')} depositors →
        </Link>
      }
    >
      {total > BigInt(0) && (
        <div className="mb-4">
          {/* R15: 28 px, the group names inside their segments, no legend row where they fit. */}
          <ConcentrationBar
            label="Share of deposits"
            assetClass="dao"
            total={total}
            groups={groups}
            labelsInside
          />
        </div>
      )}
      {/* R8: a topic page lists five rows; "All N depositors →" ranks every one. */}
      <ListTable
        label="Largest depositors"
        rows={evidence.concentration.owners.slice(0, TOPIC_ROWS)}
        columns={OWNER_COLUMNS}
        rowKey={(o) => o.lockScriptHash}
        empty="No live deposits at this indexed tip."
        mobileRow={(o) => (
          <div className="text-small space-y-1">
            <div className="flex items-baseline justify-between gap-3">
              <span className="min-w-0">
                <span className="text-text-dim mr-2">{o.rank}</span>
                {ownerLink(o)}
              </span>
              <span className="text-text-dim shrink-0">{o.sharePercent}%</span>
            </div>
            <div className="flex flex-wrap items-baseline justify-between gap-x-3 gap-y-1">
              {lockLink(o)}
              <span className="text-text-bright">
                <Amount value={o.capacity} /> CKB
                <span className="text-text-dim"> · {plural(o.depositCount, 'deposit')}</span>
              </span>
            </div>
          </div>
        )}
      />
    </Section>
  );
}

/** contracts W5-3: the block the statistics row was computed at; it must be the row's own tip. */
function statisticsAnchor(statistics: DaoStatistics): DaoStatistics['anchor'] {
  const { anchor } = statistics;
  if (!anchor || anchor.blockNumber !== statistics.tipBlockNumber)
    throw new Error(
      `DAO statistics at ${formatBlockNumber(statistics.tipBlockNumber)}: anchor ${anchor ? formatBlockNumber(anchor.blockNumber) : 'missing'} is not the block the row was computed at`
    );
  if (!/^0x[0-9a-f]{64}$/.test(anchor.blockHash))
    throw new Error(
      `DAO statistics anchor ${formatBlockNumber(anchor.blockNumber)}: ${anchor.blockHash} is not a block hash`
    );
  return anchor;
}

/**
 * Tier 2 (board V63): the header's DAO field read in CKB with AR as its exact rate, and the DAO
 * script's rules. S = still owed + burnt is stated only when the statistics and the header are the
 * same block; across two blocks it would compare two different states (DAO-16).
 */
function RawEvidence({
  evidence,
  statistics,
}: {
  evidence: DaoEvidence;
  statistics: DaoStatistics;
}) {
  const header = evidence.header;
  // DAO-16 (R2b-a 44): S, still owed and burnt from ONE statistics row at ONE block, so the
  // identity is stated whatever block the header table below comes from; contracts W5-3: that
  // block, number and hash, is the row's own `anchor`.
  const anchor = statisticsAnchor(statistics);
  const statsS = parseShannons(statistics.tipSecondaryPool, 'tipSecondaryPool');
  const owed = parseShannons(statistics.unclaimedCompensation, 'unclaimedCompensation');
  const burnt = parseShannons(statistics.burnt, 'burnt');
  if (owed + burnt !== statsS)
    throw new Error(
      `DAO statistics at ${formatBlockNumber(statistics.tipBlockNumber)}: S ${statsS} is not still owed + burnt ${owed + burnt}`
    );
  const fields = [
    {
      key: 'C',
      bytes: '0..8',
      value: (
        <>
          <Amount value={header.c} /> CKB issued in total
        </>
      ),
    },
    {
      key: 'AR',
      bytes: '8..16',
      value: <>{formatAr(header.ar)} accumulated rate (genesis 1.0)</>,
    },
    {
      key: 'S',
      bytes: '16..24',
      value: (
        <>
          <Amount value={header.s} /> CKB: compensation still owed to live deposits + the burnt
          share, which no cell can ever claim
        </>
      ),
    },
    {
      key: 'U',
      bytes: '24..32',
      value: (
        <>
          <Amount value={header.u} /> CKB occupied (incl. genesis virtual occupied)
        </>
      ),
    },
  ];
  const headerField = (
    <DetailSection
      title={
        <>
          The DAO field in every block header · <BlockLink number={header.blockNumber} />
        </>
      }
    >
      <div className="text-small">
        {fields.map((field) => (
          <div
            key={field.key}
            className="border-base-elevated grid grid-cols-[44px_96px_minmax(0,1fr)] gap-3 border-t py-1.5"
          >
            <span className="text-text-bright">{field.key}</span>
            <span className="text-text-dim">bytes {field.bytes}</span>
            <span className="min-w-0">{field.value}</span>
          </div>
        ))}
      </div>
      <div className="text-text-dim text-meta mt-3 break-all font-mono">{header.daoHex}</div>
      {/* Board v3: the explanation reads at body (13 px), as tier 2's other prose. */}
      <p className="text-text-dim text-body mt-3">
        AR, in every block header, is the DAO&apos;s growth index. Since genesis, AR grew from{' '}
        {formatAr(evidence.ar.genesisAr)} at genesis to {formatAr(evidence.ar.tipAr)} at block{' '}
        <BlockLink number={evidence.tipBlockNumber} />: free capacity deposited in block 0 would
        have grown by +{evidence.ar.sinceGenesisGrowthPercent}
        %. A deposit grows on the capacity it does not occupy; the curve flattens as more CKB
        exists. The deposit reading under &ldquo;How a deposit grows&rdquo; is the API
        calculator&apos;s for 10,000 CKB in a standard deposit cell: compensation accrues on its
        free capacity, the CKB the cell does not occupy. Compensation freezes at the withdrawal
        request AR. At block{' '}
        <Link href={`/blocks/${anchor.blockNumber}`} className="hover:text-text-bright underline">
          {formatBlockNumber(anchor.blockNumber)}
        </Link>{' '}
        (<span title={anchor.blockHash}>{truncateHash(anchor.blockHash)}</span>), S (
        <Amount value={statsS.toString()} /> CKB) equals the compensation still owed (
        <Amount value={owed.toString()} />) plus the burnt share (
        <Amount value={burnt.toString()} />) to the shannon: S only counts the burnt share, it does
        not hold it for anyone. Compensation paid + still owed equals the depositors’ share.
      </p>
      {evidence.ar.year365 && (
        <div className="text-text-dim text-meta mt-2 font-mono">
          365-day anchor: block <BlockLink number={evidence.ar.year365.anchorBlockNumber} /> · chain
          time {formatUtc8DateTime(evidence.ar.year365.anchorChainTimeMs)} · AR{' '}
          {formatAr(evidence.ar.year365.anchorAr)} · observed span{' '}
          {formatDurationMs(
            exactSafeMs(evidence.ar.year365.elapsedChainTimeMs, 'DAO 365-day observed span')
          )}{' '}
          · the last canonical block at or before the 365-day chain-time cutoff, behind
          &ldquo;Compensation, last 365 days&rdquo;
        </div>
      )}
      <Link
        href={`/blocks/${header.blockNumber}`}
        className="text-text hover:text-text-bright text-small mt-2 inline-block hover:underline"
      >
        Open source block →
      </Link>
    </DetailSection>
  );
  return (
    // "How it works ↓" in the hero lands on the evidence boundary (hierarchy D1, D8).
    <div id="evidence" className="scroll-mt-4">
      {/* The explanation column is the wider one, so its body prose keeps tier 2 short. */}
      <DetailTier2 className="grid items-start gap-x-14 gap-y-10 space-y-0 lg:grid-cols-[minmax(0,1.5fr)_minmax(0,1fr)]">
        <div className="flex min-w-0 flex-col gap-8">
          <HowItWorks statistics={statistics} evidence={evidence} />
          <DetailSection title="The DAO script">
            <dl className="text-small flex flex-col gap-2.5">
              <div>
                <dt className="text-text-dim">Type script · type</dt>
                <dd className="mt-0.5">
                  <Link
                    href="/script/0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e?hashType=type&kind=type"
                    className="text-text-bright break-all font-mono hover:underline"
                  >
                    0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e
                  </Link>
                </dd>
                <dd className="text-text-dim mt-0.5">
                  deployed in the genesis block; its code cell can never be spent
                </dd>
              </div>
              <div>
                <dt className="text-text-dim">Cell data, 8 bytes</dt>
                <dd className="mt-0.5">
                  0 for a deposit · the deposit&apos;s block number (u64 LE) for a withdrawal
                  request
                </dd>
              </div>
              <div>
                <dt className="text-text-dim">Withdrawing</dt>
                <dd className="mt-0.5 leading-relaxed">
                  header deps: the deposit block and the request block · input since: the epoch
                  where the cycle ends · paid out: (capacity − occupied) × AR(request) ÷ AR(deposit)
                  + occupied
                </dd>
              </div>
              <div>
                <dt className="text-text-dim">A standard deposit cell</dt>
                <dd className="mt-0.5">
                  102 bytes occupied with a secp256k1 lock: 8 + 53 + 33 + 8
                </dd>
              </div>
            </dl>
          </DetailSection>
        </div>
        {headerField}
      </DetailTier2>
    </div>
  );
}

/**
 * Tier 2, left column (hierarchy D8): what tier 1 states as figures, in words — the concept the
 * hero used to open with, how each block splits its issuance, how to read the sections' charts, and
 * every protocol caveat, then the statistics anchor block to verify against.
 */
function HowItWorks({
  statistics,
  evidence,
}: {
  statistics: DaoStatistics;
  evidence: DaoEvidence;
}) {
  return (
    <DetailSection title="How the Nervos DAO works">
      {/* R1 roles: the explanation and the protocol notes are prose (body, as the v3 board).
          Material pass v2 (7): the former two paragraphs are four, one topic each, and the two
          mechanisms are drawn as tiles between them; every sentence is kept, in its order. */}
      <div className="flex flex-col">
        <p className="text-text-dim text-body max-w-[64ch]">
          CKB locked in the Nervos DAO keeps its share of the supply. Secondary issuance, 1.344
          billion CKB a year, is split by how CKB is used: depositors are compensated with the share
          their deposits make up, miners get the share that is occupied, and the rest is burnt.
          Every block splits its share the same way: the proportions are simply how all CKB issued
          by that block is used right then. So a deposit keeps its share of all CKB through
          secondary issuance, while free CKB outside the DAO receives nothing. The depositors&apos;
          share counts phase-0 deposits, free capacity + compensation already accrued; withdrawal
          requests take no share, since a request&apos;s compensation stopped growing at the
          request.
        </p>
        <IssuanceSplit />
        {/* Hierarchy v3 (V7): how every block splits its issuance, moved from tier 1. */}
        <CapacityUse statistics={statistics} />
        <p className="text-text-dim text-body mt-3.5 max-w-[64ch]">
          A deposit runs in cycles of 180 epochs, about 30 days, counted from the epoch it was made.
          In &ldquo;When deposits can leave&rdquo;, each bar is the CKB whose current cycle ends
          that day: six exact epochs from the indexed tip, epoch ranges rather than predicted
          calendar dates. Epoch-lock readiness alone does not prove a complete withdraw transaction;
          the required input, headers, witnesses and script execution still apply.
        </p>
        <DepositLife />
        <p className="text-text-dim text-body mt-3.5 max-w-[64ch]">
          Protocol occupied (U) includes the genesis virtual occupied baseline; it is not the real
          occupied size of live cells. The miners&apos; share is s × U ÷ C of each block&apos;s
          secondary issuance s, paid in cellbases; the depositors&apos; share accrues via AR and is
          paid out at withdrawals. No cell can ever claim the burnt share. A hardfork could only put
          future secondary issuance to use; what is burnt stays burnt.
        </p>
        <p className="text-text-dim text-body mt-3.5 max-w-[64ch]">
          Estimated APC is at today&apos;s issuance and deposits; &ldquo;Compensation, last 365
          days&rdquo; is on free capacity, from the header&apos;s AR. Statistics are exact to the
          shannon at block <BlockLink number={statistics.tipBlockNumber} />; the indexed epoch is{' '}
          {evidence.tipEpoch.display}. &ldquo;Recent DAO activity&rdquo; lists deposits, withdrawal
          requests and withdrawals, newest first.
        </p>
      </div>
      <Link
        href={`/blocks/${statistics.tipBlockNumber}`}
        className="text-text hover:text-text-bright text-small mt-2.5 inline-block hover:underline"
      >
        Verify statistics anchor block {formatBlockNumber(statistics.tipBlockNumber)} →
      </Link>
    </DetailSection>
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
      {/* Hierarchy R6 / D9: hero → first section 64 px, sections 72 px apart, columns 56 px. */}
      <div data-tier="1" className="min-w-0">
        <Hero
          statistics={statistics}
          evidence={evidence}
          deposited={depositedHistory}
          ratio={circulationHistory}
          historyError={historyError}
        />
        <div className="mt-16 min-w-0">
          <Issuance statistics={statistics} />
        </div>
        <div className="mt-[72px] grid min-w-0 items-start gap-y-[72px] lg:grid-cols-2 lg:gap-x-14">
          <DaoGrows evidence={evidence} />
          <Windows evidence={evidence} />
        </div>
        <div className="mt-[72px] min-w-0">
          <Owners evidence={evidence} />
        </div>
        <div className="mt-[72px] min-w-0">
          <DaoSources />
        </div>
      </div>
      <RawEvidence evidence={evidence} statistics={statistics} />
    </div>
  );
}
