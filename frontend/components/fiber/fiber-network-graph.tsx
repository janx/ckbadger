'use client';

import { useMemo, type ReactNode } from 'react';
import Link from '@/components/ui/link';
import { ReadoutRow } from '@/components/ui/readout-row';
import { useElementWidth } from '@/hooks/useElementWidth';
import { usePinnedReadout } from '@/hooks/usePinnedReadout';
import { ExactCkb } from '@/components/activity-event-row';
import type { FiberNetwork, FiberParty } from '@/lib/api';
import { formatBlockNumber } from '@/lib/format-block';
import { formatCount } from '@/lib/format-count';
import { formatDurationMs, formatUtc8Date } from '@/lib/utils';
import { FIBER_COLORS } from '@/lib/design-tokens';
import {
  fiberGraphData,
  fiberGraphLayout,
  partyTail,
  type FiberGraphEdge,
  type FiberGraphModel,
  type FiberGraphNode,
} from '@/lib/fiber-network-model';
import { tapOrHover } from '@/components/ui/tap-or-hover-hint';

interface Props {
  network: FiberNetwork;
}

type Active = { kind: 'node'; id: string } | { kind: 'edge'; id: string };

const DIM = 0.18;
const activeKey = (target: Active) => `${target.kind}:${target.id}`;

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
      <div className="text-text-bright text-[13.5px]">
        <PartyLink party={party} />
      </div>
      <div className="text-text-dim mb-2 mt-0.5 text-[11.5px]" style={{ overflowWrap: 'anywhere' }}>
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
      <div className="text-text-bright text-[13.5px]">
        …{partyTail(a)} ↔ …{partyTail(b)}
      </div>
      <div className="text-text-dim mb-2 mt-0.5 text-[11.5px]" style={{ overflowWrap: 'anywhere' }}>
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
function readoutLine(model: FiberGraphModel, active: Active): string {
  const node = (id: string) => model.nodes.find((n) => n.id === id)!;
  if (active.kind === 'node') {
    const party = node(active.id);
    const partners = model.edges
      .filter((e) => e.a === party.id || e.b === party.id)
      .map((e) => ({ label: node(e.a === party.id ? e.b : e.a).label, channels: e.pair.channels }))
      .sort((p, q) => q.channels - p.channels || p.label.localeCompare(q.label))
      .map((p) => `${p.label} ${p.channels.toLocaleString('en-US')}`);
    const head = `${party.label} · ${formatCount(party.channels, 'channel')}`;
    return partners.length ? `${head} · pairs with ${partners.join(', ')}` : head;
  }
  const edge = model.edges.find((e) => e.id === active.id)!;
  const { pair } = edge;
  const forced = pair.forceClosedSettledChannels + pair.forceClosedWaitingChannels;
  return [
    `…${node(edge.a).label} ↔ …${node(edge.b).label}`,
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
 * "The network, as the chain sees it" (Fiber board V65): parties as nodes, pairs as edges. The
 * layout is computed for the measured column (R4) and drawn 1:1; a tap pins a party or a pair in
 * the line under the graph (R6) as well as in the side panel.
 */
export function FiberNetworkGraph({ network }: Props) {
  const data = useMemo(() => fiberGraphData(network), [network]);
  const [box, measured] = useElementWidth<HTMLDivElement>();
  const width = measured === null ? null : Math.floor(measured);
  const model = useMemo(
    () => (width === null ? null : fiberGraphLayout(data, width)),
    [data, width]
  );
  const readout = usePinnedReadout<Active>(activeKey);
  const active = readout.active;
  const nodesById = useMemo(() => new Map((model?.nodes ?? []).map((n) => [n.id, n])), [model]);
  const edgesById = useMemo(() => new Map((model?.edges ?? []).map((e) => [e.id, e])), [model]);
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
  const handlers = (target: Active) => ({
    ...readout.bind(target),
    className: 'cursor-pointer outline-none',
  });

  return (
    <section aria-labelledby="fiber-network-heading" className="flex min-w-0 flex-col gap-3">
      <div className="flex flex-wrap items-baseline justify-between gap-4">
        <h2 id="fiber-network-heading" className="text-text-bright text-[15px] font-medium">
          The network, as the chain sees it
        </h2>
        <span className="text-text-dim flex flex-wrap gap-x-[18px] gap-y-1 text-[11.5px]">
          <span className="inline-flex items-center gap-1.5">
            <LegendLine stroke={FIBER_COLORS.open} width={3} />
            CKB locked now
          </span>
          <span className="inline-flex items-center gap-1.5">
            <LegendLine stroke={FIBER_COLORS.history} width={3} opacity={0.4} />
            channels over time
          </span>
          <span className="inline-flex items-center gap-1.5">
            <LegendLine stroke={FIBER_COLORS.forced} width={1} dash="3 3" />
            force-closed
          </span>
          <span className="inline-flex items-center gap-1.5">
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
          </span>
        </span>
      </div>
      {network.parties.length === 0 ? (
        <p className="text-text-dim py-10 text-[13px]">No on-chain parties observed yet.</p>
      ) : (
        <div className="grid min-w-0 items-start gap-7 lg:grid-cols-[minmax(0,1fr)_340px]">
          <div className="flex min-w-0 flex-col gap-2">
            <div ref={box} className="min-w-0">
              {model && (
                <svg
                  data-fiber-graph
                  viewBox={`0 0 ${model.width} ${model.height}`}
                  width={model.width}
                  height={model.height}
                  role="img"
                  aria-label={`The Fiber network on chain: ${model.nodes.length} parties and the ${model.edges.length} pairs that opened channels`}
                  className="block overflow-visible"
                >
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
                </svg>
              )}
            </div>
            <ReadoutRow
              label="Party or pair readout"
              pinned={readout.showingPin}
              idle={tapOrHover('a party or a pair.', { capital: true })}
            >
              {model && active ? readoutLine(model, active) : null}
            </ReadoutRow>
          </div>
          <aside
            aria-label="Party or pair"
            aria-live="polite"
            className="border-base-border-subtle min-h-[220px] min-w-0 rounded-md border px-3.5 py-3 text-[12px] lg:sticky lg:top-4"
          >
            {activeNode ? (
              <PartyPanel node={activeNode} partners={data.neighbours.get(activeNode.id)!.size} />
            ) : activeEdge ? (
              <PairPanel
                edge={activeEdge}
                a={nodesById.get(activeEdge.a)!.party}
                b={nodesById.get(activeEdge.b)!.party}
              />
            ) : (
              <div>
                <div className="text-text-bright text-[13.5px]">
                  {tapOrHover('a party or a pair', { capital: true })}
                </div>
                <p className="text-text mt-1.5 leading-relaxed">
                  Each circle is a party: the address that funded or received a channel. Its size
                  follows how many channels it took part in. A green ring means it has a channel
                  open now.
                </p>
                <p className="text-text mt-1.5 leading-relaxed">
                  Each line is a pair that opened channels. Grey grows with how many channels they
                  opened over time; green with the CKB locked between them right now; a dashed line
                  means a channel was force-closed.
                </p>
              </div>
            )}
          </aside>
        </div>
      )}
      <p className="text-text-dim max-w-[900px] text-[12px]">
        Parties are on-chain addresses, not Fiber node ids: the chain shows who funded a channel,
        not which node ran it.
        {data.unpairedChannels > 0 &&
          ` ${formatCount(data.unpairedChannels, 'channel')} with more or fewer than two parties ${data.unpairedChannels === 1 ? 'is' : 'are'} not drawn.`}
      </p>
    </section>
  );
}
