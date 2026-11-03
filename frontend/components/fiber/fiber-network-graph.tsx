'use client';

import { useMemo, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { Prose } from '@/components/ui/prose';
import { ReadoutRow } from '@/components/ui/readout-row';
import { TerminalPanel, TerminalPanelContent } from '@/components/ui/terminal-panel';
import { STICKY_RAIL } from '@/components/layout/sticky-header';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { ExactCkb } from '@/components/activity-event-row';
import type { PinnedReadout } from '@/hooks/usePinnedReadout';
import type { FiberNetwork, FiberParty } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { formatDurationMs, formatUtc8Date } from '@/lib/utils';
import { FIBER_COLORS } from '@/lib/design-tokens';
import {
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

/** A party (node) or a pair (edge) of the graph: what a hover shows and a tap pins. */
export type FiberGraphTarget = { kind: 'node'; id: string } | { kind: 'edge'; id: string };

interface Props {
  network: FiberNetwork;
  /** The page's pin (`useFiberGraphPin`), shared by the picture and its reading. */
  pin: PinnedReadout<FiberGraphTarget>;
}

const DIM = 0.18;
const targetKey = (target: FiberGraphTarget) => `${target.kind}:${target.id}`;
/** The band at the plot's top that holds its name (one 11 px line and its gap, R13). */
const NAME_BAND = 22;

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
  // the islands' note: an 11 px line, 3 px of descenders
  if (model.islandsNote) bottom = Math.max(bottom, model.islandsNote.y + 3);
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

function PartyLink({ party }: { party: FiberParty }) {
  const text = party.address
    ? `${party.address.slice(0, 10)}…${party.address.slice(-6)}`
    : `${party.lockScriptHash.slice(0, 10)}…${party.lockScriptHash.slice(-6)}`;
  return party.address ? (
    <Link href={`/address/${party.address}`} className="text-text-bright hover:text-jade">
      {text}
    </Link>
  ) : (
    <span className="text-text-bright" title={party.lockScriptHash}>
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
      {count} · <ExactCkb shannons={BigInt(capacity)} signed={false} tone="plain" />
    </>
  );
}

function PartyPanel({ node, partners }: { node: FiberGraphNode; partners: number }) {
  const { party } = node;
  return (
    <div>
      <div className="text-text-bright text-body">
        <PartyLink party={party} />
      </div>
      <div className="text-text-dim text-meta mb-2 mt-0.5" style={{ overflowWrap: 'anywhere' }}>
        {party.lockScriptName ? `${party.lockScriptName} · ` : ''}lock hash{' '}
        {party.lockScriptHash.slice(0, 10)}…{party.lockScriptHash.slice(-6)}
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
            `${party.forceClosedSettledChannels} settled · ${party.forceClosedWaitingChannels} not yet`,
          ],
          [
            'first channel',
            <span key="first">
              {formatUtc8Date(party.firstOpenTimestamp)} ·{' '}
              <Link href={`/blocks/${party.firstOpenBlock}`} className="hover:text-jade">
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
      <div className="text-text-dim text-meta mb-2 mt-0.5" style={{ overflowWrap: 'anywhere' }}>
        {formatCount(pair.channels, 'channel')} between <PartyLink party={a} /> and{' '}
        <PartyLink party={b} />
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
            `${pair.forceClosedSettledChannels} settled · ${pair.forceClosedWaitingChannels} not yet`,
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
              <Link href={`/blocks/${pair.firstOpenBlock}`} className="hover:text-jade">
                {formatBlockNumber(pair.firstOpenBlock)}
              </Link>
              {' – '}
              <Link href={`/blocks/${pair.lastOpenBlock}`} className="hover:text-jade">
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

/** The board's legend, at the top of the column beside the plot (its top right). */
function GraphKey() {
  const item = 'mr-[18px] inline-flex items-center gap-1.5 last:mr-0';
  return (
    <ul aria-label="Graph key" className="text-meta text-text-dim">
      <li className={item}>
        <LegendLine stroke={FIBER_COLORS.open} width={3} />
        CKB locked now
      </li>
      <li className={item}>
        <LegendLine stroke={FIBER_COLORS.history} width={3} opacity={0.4} />
        channels over time
      </li>
      <li className={item}>
        <LegendLine stroke={FIBER_COLORS.forced} width={1} dash="3 3" />
        force-closed
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
    return <p className="text-text-dim text-body py-10">No on-chain parties observed yet.</p>;
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
    className: 'cursor-pointer outline-none',
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
                aria-label={`The Fiber network on chain: ${model.nodes.length} parties and the ${model.edges.length} pairs that opened channels`}
                className="block"
              >
                <text data-role="chart-name" x="8" y="14" fontSize="11" fill={FIBER_COLORS.note}>
                  the network, as the chain sees it
                </text>
                <g data-role="drawing" transform={`translate(0 ${shift})`}>
                  {model.islandsNote && (
                    <text
                      x={model.islandsNote.x}
                      y={model.islandsNote.y}
                      textAnchor={model.islandsNote.anchor}
                      fontSize="11"
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
                          x1={edge.x1}
                          y1={edge.y1}
                          x2={edge.x2}
                          y2={edge.y2}
                          stroke={FIBER_COLORS.history}
                          strokeOpacity=".28"
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
                          y={node.y + node.r + 14}
                          textAnchor="middle"
                          fontSize="11"
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
                            y={node.y + 4}
                            textAnchor="middle"
                            fontSize="11"
                            fill={FIBER_COLORS.count}
                          >
                            {node.channels}
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
            className="border-base-border-subtle mt-3 rounded-md border px-3.5 py-3"
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
      <p data-role="caption" className="text-text text-body mt-3.5 max-w-[640px]">
        Each circle is a party, each line a pair; green is CKB locked between them now.
      </p>
    </>
  );
}

/** Tier 2, "How to read the graph": the explainer the picture's column used to hold, as Prose. */
export function FiberGraphNotes() {
  return (
    <Prose>
      <p>
        Each circle is a party: the address that funded or received a channel. Its size follows how
        many channels it took part in. A green ring means it has a channel open now.
      </p>
      <p>
        Each line is a pair that opened channels. Grey grows with how many channels they opened over
        time; green with the CKB locked between them right now; a dashed line means a channel was
        force-closed.
      </p>
    </Prose>
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
      {data.unpairedChannels > 0 &&
        ` ${formatCount(data.unpairedChannels, 'channel')} with more or fewer than two parties ${data.unpairedChannels === 1 ? 'is' : 'are'} not drawn.`}
    </p>
  );
}
