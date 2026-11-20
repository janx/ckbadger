'use client';

import { useMemo, type ReactNode } from 'react';
import Link, { type LinkLook } from '@/components/ui/link';
import { ReadoutRow } from '@/components/ui/readout-row';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { STICKY_RAIL } from '@/components/layout/sticky-header';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { CkbAmount } from '@/components/ui/ckb-amount';
import type { PinnedReadout } from '@/hooks/usePinnedReadout';
import type { FiberNetwork, FiberParty } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount, countNoun } from '@/lib/format-count';
import { formatDurationMs, formatUtc8Date } from '@/lib/utils';
import { FIBER_COLORS } from '@/lib/design-tokens';
import {
  FIBER_GRAPH,
  fiberGraphData,
  fiberGraphLayout,
  fiberNodeReach,
  pairKey,
  partyTail,
  type FiberGraphData,
  type FiberGraphEdge,
  type FiberGraphModel,
  type FiberGraphNode,
} from '@/lib/fiber-network-model';
import { Utc8Date } from '@/components/ui/utc8-date';
import { ShortId } from '@/components/ui/short-id';
import { unitNode } from '@/components/ui/units';
import { SVG_TEXT, centreShift, lineAscentPx } from '@/lib/chart-type';
import { descentPx, tightLinePx } from '@/lib/type-metrics';
import { partyLockWordsText } from '@/lib/lock-words';

/** A party (node) or a pair (edge) of the graph: what a hover shows and a tap pins. */
export type FiberGraphTarget = { kind: 'node'; id: string } | { kind: 'edge'; id: string };

interface Props {
  network: FiberNetwork;
  /** The page's pin (`useFiberGraphPin`), shared by the picture and its reading. */
  pin: PinnedReadout<FiberGraphTarget>;
}

const DIM = 0.18;
/**
 * The strength of a pair's grey line (its channels over time), in the figure and in its key alike
 * (lane P4; review fiber-channels: the grey edges all but vanished by night). The night reading ink
 * at 0.28 on the page ground makes about 2.0 : 1; at 0.4, 2.86 : 1, and the key already drew it at
 * 0.4 while the figure drew 0.28.
 */
const HISTORY_OPACITY = 0.4;
const targetKey = (target: FiberGraphTarget) => `${target.kind}:${target.id}`;
/** The plot's name, a label line 2 px under the svg's top. */
const NAME_PAD = 2;
const NAME_Y = NAME_PAD + lineAscentPx(SVG_TEXT.label);
/** The band at the plot's top that holds its name: its line and 5 px under it (22 px, R13). */
const NAME_BAND = NAME_PAD + tightLinePx(SVG_TEXT.label) + 5;

/**
 * The box the picture needs (material pass v2 leftovers: the graph sits in the instrument frame,
 * clipped to its svg, so the svg must hold everything it draws): the drawing's bounds from every
 * party's reach — its ring or label across, its ring up, its label down — and from the notes.
 * `shift` moves the drawing down only as far as a party would reach into the name's band;
 * `height` is the model's or the lowest label's, whichever is lower, plus the shift. Across, the
 * layout's contract is that nothing reaches past the width; a party that does is named.
 */
export function fiberPictureBox(model: FiberGraphModel): { shift: number; height: number } {
  let top = Infinity;
  let bottom = 0;
  for (const node of model.nodes) {
    const reach = fiberNodeReach(node.r);
    if (node.x - reach.x < 0 || node.x + reach.x > model.width)
      throw new Error(
        `Fiber graph: party …${node.label} reaches past the ${model.width} px canvas (x ${node.x.toFixed(1)}, reach ${reach.x.toFixed(1)})`
      );
    top = Math.min(top, node.y - reach.up);
    bottom = Math.max(bottom, node.y + reach.down);
  }
  // the islands' note: a label line, its descent under its baseline
  if (model.islandsNote) bottom = Math.max(bottom, model.islandsNote.y + descentPx(SVG_TEXT.label));
  const shift = Math.max(0, NAME_BAND - top);
  return { shift, height: Math.ceil(Math.max(model.height, bottom)) + shift };
}
/** The reading's line while nothing is hovered or pinned: what it will read (R14: the row's own
 * "tap or hover" is the section's one hint). */
const IDLE = 'a party’s channels and partners, or a pair’s channels';

/** The hover / tap state of the graph, held by the page so its reading can sit under the hero. */
export function useFiberGraphPin(): PinnedReadout<FiberGraphTarget> {
  return usePinnedReadout<FiberGraphTarget>(targetKey);
}

/**
 * A party in the graph's reading panel, at the row size: its address, else its lock hash (U-09);
 * the panel's key where it heads the panel, a link inside the line where a pair's line names it.
 */
function PartyLink({ party, look }: { party: FiberParty; look: LinkLook }) {
  const text = party.address ? (
    <ShortId value={party.address} kind="address" size="row" />
  ) : (
    <ShortId value={party.lockScriptHash} kind="hash" size="row" />
  );
  return party.address ? (
    <Link href={`/address/${party.address}`} look={look}>
      {text}
    </Link>
  ) : (
    <span className={look === 'key' ? 'text-text-bright' : undefined} title={party.lockScriptHash}>
      {text}
    </span>
  );
}

function Rows({ rows }: { rows: [string, ReactNode, string?][] }) {
  return (
    <dl className="mt-2">
      {rows.map(([label, value, tone]) => (
        <div key={label} className="grid grid-cols-[112px_minmax(0,1fr)] gap-2 py-[3px]">
          <dt className="text-text-dim">{label}</dt>
          <dd className={tone ?? 'text-text'} style={{ overflowWrap: 'anywhere' }}>
            {value}
          </dd>
        </div>
      ))}
    </dl>
  );
}

function OpenNow({ count, capacity }: { count: number; capacity: string }) {
  if (count === 0) return <>none</>;
  return (
    <>
      {count.toLocaleString('en-US')} ·{' '}
      <CkbAmount form="summary" shannons={BigInt(capacity)} signed={false} tone="plain" />
    </>
  );
}

function PartyPanel({ node, partners }: { node: FiberGraphNode; partners: number }) {
  const { party } = node;
  return (
    <div>
      <div className="text-text-bright text-body">
        <PartyLink party={party} look="key" />
      </div>
      <div className="text-text-dim text-meta mb-2 mt-0.5" style={{ overflowWrap: 'anywhere' }}>
        {/* the party's lock by the one decision (the lead's final round, item 7): the lock its
            address encodes with the registry's name the route sends for it; the route reads both
            from one stored lock script, so a party with no address has no name: not indexed */}
        {partyLockWordsText(party, 'row')} · lock hash{' '}
        <ShortId value={party.lockScriptHash} kind="hash" size="row" />
      </div>
      <Rows
        rows={[
          [
            'channels',
            `${party.channels.toLocaleString('en-US')} with ${formatCount(partners, 'party', 'parties')}`,
            'text-text-bright',
          ],
          [
            'open now',
            <OpenNow key="open" count={party.openChannels} capacity={party.openCapacity} />,
            party.openChannels ? 'text-jade' : 'text-text-dim',
          ],
          ['closed together', party.cooperativelyClosedChannels.toLocaleString('en-US')],
          [
            'force-closed',
            `${party.forceClosedSettledChannels.toLocaleString('en-US')} settled · ${party.forceClosedWaitingChannels.toLocaleString('en-US')} not yet`,
          ],
          [
            'first channel',
            <span key="first">
              <Utc8Date timestamp={party.firstOpenTimestamp} /> ·{' '}
              <Link href={`/blocks/${party.firstOpenBlock}`} look="inline">
                {formatBlockNumber(party.firstOpenBlock)}
              </Link>
            </span>,
          ],
        ]}
      />
    </div>
  );
}

function PairPanel({ edge, a, b }: { edge: FiberGraphEdge; a: FiberParty; b: FiberParty }) {
  const { pair } = edge;
  return (
    <div>
      <div className="text-text-bright text-body">
        …{partyTail(a)} ↔ …{partyTail(b)}
      </div>
      {/* The pair's parties are values and links: the reading ink at the panel's small step (they
          were a quiet 12 px line). */}
      <div className="text-text mb-2 mt-0.5" style={{ overflowWrap: 'anywhere' }}>
        {unitNode(formatCount(pair.channels, 'channel'))} between{' '}
        <PartyLink party={a} look="inline" /> and <PartyLink party={b} look="inline" />
      </div>
      <Rows
        rows={[
          [
            'open now',
            <OpenNow key="open" count={pair.openChannels} capacity={pair.openCapacity} />,
            pair.openChannels ? 'text-jade' : 'text-text-dim',
          ],
          ['closed together', pair.cooperativelyClosedChannels.toLocaleString('en-US')],
          [
            'force-closed',
            `${pair.forceClosedSettledChannels.toLocaleString('en-US')} settled · ${pair.forceClosedWaitingChannels.toLocaleString('en-US')} not yet`,
          ],
          [
            'typical lifetime',
            pair.typicalClosedLifetimeMs === null
              ? 'none closed yet'
              : formatDurationMs(Number(pair.typicalClosedLifetimeMs)),
          ],
          [
            'first · last',
            `${formatUtc8Date(pair.firstOpenTimestamp)} · ${formatUtc8Date(pair.lastOpenTimestamp)}`,
          ],
          [
            'opened in',
            <span key="blocks">
              <Link href={`/blocks/${pair.firstOpenBlock}`} look="inline">
                {formatBlockNumber(pair.firstOpenBlock)}
              </Link>
              {' – '}
              <Link href={`/blocks/${pair.lastOpenBlock}`} look="inline">
                {formatBlockNumber(pair.lastOpenBlock)}
              </Link>
            </span>,
          ],
        ]}
      />
    </div>
  );
}

/** The one-line reading under the graph: a party with its partners, or a pair (R6). */
function readoutLine(data: FiberGraphData, active: FiberGraphTarget): string {
  const party = (id: string) => data.byId.get(id)!;
  if (active.kind === 'node') {
    const self = party(active.id);
    const partners = data.pairs
      .filter((p) => p.firstLockScriptHash === active.id || p.secondLockScriptHash === active.id)
      .map((p) => ({
        label: partyTail(
          party(
            p.firstLockScriptHash === active.id ? p.secondLockScriptHash : p.firstLockScriptHash
          )
        ),
        channels: p.channels,
      }))
      .sort((p, q) => q.channels - p.channels || p.label.localeCompare(q.label))
      .map((p) => `${p.label} ${p.channels.toLocaleString('en-US')}`);
    const head = `${partyTail(self)} · ${formatCount(self.channels, 'channel')}`;
    return partners.length ? `${head} · pairs with ${partners.join(', ')}` : head;
  }
  const pair = data.pairs.find((p) => pairKey(p) === active.id)!;
  const forced = pair.forceClosedSettledChannels + pair.forceClosedWaitingChannels;
  return [
    `…${partyTail(party(pair.firstLockScriptHash))} ↔ …${partyTail(party(pair.secondLockScriptHash))}`,
    formatCount(pair.channels, 'channel'),
    `${pair.openChannels.toLocaleString('en-US')} open now`,
    ...(forced ? [`${forced.toLocaleString('en-US')} force-closed`] : []),
  ].join(' · ');
}

function LegendLine({
  stroke,
  width,
  dash,
  opacity,
}: {
  stroke: string;
  width: number;
  dash?: string;
  opacity?: number;
}) {
  return (
    <svg width="22" height="8" aria-hidden="true">
      <line
        x1="0"
        y1="4"
        x2="22"
        y2="4"
        stroke={stroke}
        strokeWidth={width}
        strokeDasharray={dash}
        strokeOpacity={opacity}
      />
    </svg>
  );
}

/**
 * The graph's key, at the top of the column beside the plot (its top right; under it below lg):
 * every mark the figure draws, drawn as the figure draws it, and what it stands for (the plan's P4
 * sentence: the graph's numbers and sizes come up from tier 2 as a legend, not as prose; they were
 * the tier-2 "How to read the graph"). A party's circle grows with its channels and a large one
 * carries their count; a pair's grey line widens with the channels it opened over time, its green
 * line with the CKB locked between the two now. The words are labels: the meta step, the quiet
 * ink. The widest item, the party's, is a 22 px glyph, 6 px and 37 characters at 12 px (266.4):
 * 294.4 px, inside the 340 px column and a 358 px phone's line.
 */
function GraphKey() {
  const item = 'mr-[18px] inline-flex items-center gap-1.5 last:mr-0';
  return (
    <ul aria-label="Graph key" className="text-meta text-text-dim">
      <li className={item}>
        <svg width="22" height="22" aria-hidden="true">
          <circle
            cx="11"
            cy="11"
            r="10"
            fill={FIBER_COLORS.nodeFill}
            stroke={FIBER_COLORS.nodeStroke}
            strokeWidth="1.2"
          />
          <text
            x="11"
            y={11 + centreShift(SVG_TEXT.label)}
            textAnchor="middle"
            fontSize={SVG_TEXT.label}
            fill={FIBER_COLORS.count}
          >
            9
          </text>
        </svg>
        party · size and number: its channels
      </li>
      <li className={item}>
        <svg width="14" height="14" aria-hidden="true">
          <circle
            cx="7"
            cy="7"
            r="4"
            fill={FIBER_COLORS.nodeFill}
            stroke={FIBER_COLORS.nodeStroke}
          />
          <circle cx="7" cy="7" r="6.2" fill="none" stroke={FIBER_COLORS.open} />
        </svg>
        has an open channel
      </li>
      <li className={item}>
        <LegendLine stroke={FIBER_COLORS.history} width={3} opacity={HISTORY_OPACITY} />
        pair · grey width: channels over time
      </li>
      <li className={item}>
        <LegendLine stroke={FIBER_COLORS.open} width={3} opacity={0.85} />
        green width: CKB locked now
      </li>
      <li className={item}>
        <LegendLine stroke={FIBER_COLORS.forced} width={1} dash="3 3" opacity={0.7} />
        force-closed
      </li>
    </ul>
  );
}

/**
 * The Fiber page's picture (board V65; hierarchy v3, R13): parties as nodes, pairs as edges, the
 * name inside the plot at its top left and the legend at its top right. The layout is computed
 * for the measured column (R4) and drawn 1:1; a hover or a tap shows a party or a pair in the
 * column beside it, and in the reading under the hero (`FiberNetworkReading`).
 */
export function FiberNetworkPicture({ network, pin }: Props) {
  const data = useMemo(() => fiberGraphData(network), [network]);
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const width = measured === null ? null : Math.floor(measured);
  const model = useMemo(
    () => (width === null || data.parties.length === 0 ? null : fiberGraphLayout(data, width)),
    [data, width]
  );
  const active = pin.active;
  const nodesById = useMemo(() => new Map((model?.nodes ?? []).map((n) => [n.id, n])), [model]);
  const edgesById = useMemo(() => new Map((model?.edges ?? []).map((e) => [e.id, e])), [model]);
  if (network.parties.length === 0)
    return <p className="text-text text-body py-10">No on-chain parties observed yet.</p>;
  const activeEdge = active?.kind === 'edge' ? edgesById.get(active.id) : undefined;
  const activeNode = active?.kind === 'node' ? nodesById.get(active.id) : undefined;
  const litNodes = activeNode
    ? new Set([activeNode.id, ...data.neighbours.get(activeNode.id)!])
    : activeEdge
      ? new Set([activeEdge.a, activeEdge.b])
      : null;
  const edgeLit = (edge: FiberGraphEdge) =>
    activeNode
      ? edge.a === activeNode.id || edge.b === activeNode.id
      : activeEdge
        ? edge.id === activeEdge.id
        : true;
  const handlers = (target: FiberGraphTarget) => ({
    ...pin.bind(target),
    className: 'cursor-pointer',
  });
  const picture = model ? fiberPictureBox(model) : null;
  const shift = picture?.shift ?? 0;

  return (
    <div className="grid min-w-0 items-start gap-x-7 gap-y-4 lg:grid-cols-[minmax(0,1fr)_340px]">
      {/* Material pass v2 leftovers: the picture is an instrument, in the one frame (as the network
          map). Measured inside the frame from mount, so it is drawn 1:1 in its column at every
          width; its svg holds everything it draws (fiberPictureBox), so it clips nothing. */}
      <TerminalPanel aria-label="The Fiber network on chain" className="min-w-0 self-start">
        <TerminalPanelContent padding="sm">
          <div ref={box} className="min-w-0">
            {model && picture && (
              <svg
                data-fiber-graph
                viewBox={`0 0 ${model.width} ${picture.height}`}
                width={model.width}
                height={picture.height}
                role="img"
                aria-label={`The Fiber network on chain: ${formatCount(model.nodes.length, 'party', 'parties')} and the ${formatCount(model.edges.length, 'pair')} that opened channels`}
                className="block"
              >
                <text
                  data-role="chart-name"
                  x="8"
                  y={NAME_Y}
                  fontSize={SVG_TEXT.label}
                  fill={FIBER_COLORS.note}
                >
                  the network, as the chain sees it
                </text>
                <g data-role="drawing" transform={`translate(0 ${shift})`}>
                  {model.islandsNote && (
                    <text
                      x={model.islandsNote.x}
                      y={model.islandsNote.y}
                      textAnchor={model.islandsNote.anchor}
                      fontSize={SVG_TEXT.label}
                      fill={FIBER_COLORS.note}
                    >
                      not linked to the rest
                    </text>
                  )}
                  {model.edges.map((edge) => {
                    const lit = edgeLit(edge);
                    const [a, b] = [nodesById.get(edge.a)!.party, nodesById.get(edge.b)!.party];
                    return (
                      <g
                        key={edge.id}
                        data-edge
                        data-lit={lit}
                        opacity={lit ? 1 : DIM}
                        aria-label={`Pair …${partyTail(a)} and …${partyTail(b)}, ${formatCount(edge.pair.channels, 'channel')}`}
                        {...handlers({ kind: 'edge', id: edge.id })}
                      >
                        <line
                          x1={edge.x1}
                          y1={edge.y1}
                          x2={edge.x2}
                          y2={edge.y2}
                          stroke="transparent"
                          strokeWidth="12"
                        />
                        <line
                          data-history
                          x1={edge.x1}
                          y1={edge.y1}
                          x2={edge.x2}
                          y2={edge.y2}
                          stroke={FIBER_COLORS.history}
                          strokeOpacity={HISTORY_OPACITY}
                          strokeWidth={edge.greyWidth.toFixed(2)}
                        />
                        {edge.jadeWidth !== null && (
                          <line
                            data-open
                            x1={edge.x1}
                            y1={edge.y1}
                            x2={edge.x2}
                            y2={edge.y2}
                            stroke={FIBER_COLORS.open}
                            strokeOpacity=".85"
                            strokeWidth={edge.jadeWidth.toFixed(2)}
                          />
                        )}
                        {edge.forceClosed && (
                          <line
                            data-forced
                            {...edge.dash}
                            stroke={FIBER_COLORS.forced}
                            strokeOpacity=".7"
                            strokeWidth="1"
                            strokeDasharray="3 3"
                          />
                        )}
                      </g>
                    );
                  })}
                  {model.nodes.map((node) => {
                    const lit = litNodes ? litNodes.has(node.id) : true;
                    return (
                      <g
                        key={node.id}
                        data-node
                        data-lit={lit}
                        opacity={lit ? 1 : DIM}
                        aria-label={`Party …${node.label}, ${formatCount(node.channels, 'channel')}${node.openChannels ? `, ${node.openChannels} open` : ''}`}
                        {...handlers({ kind: 'node', id: node.id })}
                      >
                        <circle cx={node.x} cy={node.y} r={node.r + 8} fill="transparent" />
                        {node.ring && (
                          <circle
                            cx={node.x}
                            cy={node.y}
                            r={node.r + 3}
                            fill="none"
                            stroke={FIBER_COLORS.open}
                            strokeWidth="1.2"
                            strokeOpacity=".9"
                          />
                        )}
                        <circle
                          cx={node.x}
                          cy={node.y}
                          r={node.r}
                          fill={FIBER_COLORS.nodeFill}
                          stroke={FIBER_COLORS.nodeStroke}
                          strokeWidth="1.2"
                        />
                        <text
                          x={node.x}
                          y={node.y + node.r + FIBER_GRAPH.labelDrop}
                          textAnchor="middle"
                          fontSize={SVG_TEXT.label}
                          fill={FIBER_COLORS.label}
                          stroke={FIBER_COLORS.halo}
                          strokeWidth="3"
                          paintOrder="stroke"
                        >
                          {node.label}
                        </text>
                        {node.r > 14 && (
                          <text
                            x={node.x}
                            y={node.y + centreShift(SVG_TEXT.label)}
                            textAnchor="middle"
                            fontSize={SVG_TEXT.label}
                            fill={FIBER_COLORS.count}
                          >
                            {node.channels.toLocaleString('en-US')}
                          </text>
                        )}
                      </g>
                    );
                  })}
                </g>
              </svg>
            )}
          </div>
        </TerminalPanelContent>
      </TerminalPanel>
      <aside
        aria-label="Party or pair"
        aria-live="polite"
        className={`text-small min-w-0 ${STICKY_RAIL}`}
      >
        <GraphKey />
        {(activeNode || activeEdge) && (
          <div
            data-role="party-or-pair"
            className="border-base-border-subtle mt-3 border px-3.5 py-3"
          >
            {activeNode ? (
              <PartyPanel node={activeNode} partners={data.neighbours.get(activeNode.id)!.size} />
            ) : (
              <PairPanel
                edge={activeEdge!}
                a={nodesById.get(activeEdge!.a)!.party}
                b={nodesById.get(activeEdge!.b)!.party}
              />
            )}
          </div>
        )}
      </aside>
    </div>
  );
}

/**
 * Under the hero section (R13): the graph's one reading line — the hovered or tapped party or
 * pair — and its one caption.
 */
export function FiberNetworkReading({ network, pin }: Props) {
  const data = useMemo(() => fiberGraphData(network), [network]);
  if (data.parties.length === 0) return null;
  return (
    <>
      <ReadoutRow label="Party or pair readout" pinned={pin.showingPin} idle={IDLE}>
        {pin.active ? readoutLine(data, pin.active) : null}
      </ReadoutRow>
      <p data-role="caption" className="text-text text-body max-w-measure mt-3.5">
        Each circle is a party, each line a pair; green is the CKB they lock now.
      </p>
    </>
  );
}

/** Tier 2: what a party is, and the channels the graph cannot draw as a pair. One paragraph, set
 * in the page's "What the chain sees of Fiber" Prose: it carries no classes of its own. */
export function FiberPartiesNote({ network }: { network: FiberNetwork }) {
  const data = useMemo(() => fiberGraphData(network), [network]);
  return (
    <p>
      Parties are on-chain addresses, not Fiber node ids: the chain shows who funded a channel, not
      which node ran it.
      {unitNode(
        data.unpairedChannels > 0 &&
          ` ${formatCount(data.unpairedChannels, 'channel')} with more or fewer than two parties ${countNoun(data.unpairedChannels, 'is', 'are')} not drawn.`
      )}
    </p>
  );
}
