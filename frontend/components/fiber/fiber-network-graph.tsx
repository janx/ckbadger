'use client';

import { useState } from 'react';
import Link from '@/components/ui/link';
import type { FiberNetwork, FiberPair } from '@/lib/api';
import { formatCkbAmount } from '@/lib/utils';
import { formatBlockNumber } from '@/lib/format-block';

interface Props {
  network: FiberNetwork;
}

const short = (value: string) => `${value.slice(0, 8)}…${value.slice(-6)}`;

export function FiberNetworkGraph({ network }: Props) {
  const [selected, setSelected] = useState<FiberPair | null>(null);
  const parties = network.parties;
  const representedChannels = network.pairs.reduce((total, pair) => total + pair.channels, 0);
  if (representedChannels > network.summary.totalChannels) {
    throw new Error('Fiber pair channel count exceeds total observed channels');
  }
  const unpairedChannels = network.summary.totalChannels - representedChannels;
  const positions = new Map(
    parties.map((party, index) => {
      const angle = (2 * Math.PI * index) / Math.max(parties.length, 1) - Math.PI / 2;
      return [
        party.lockScriptHash,
        { x: 430 + 350 * Math.cos(angle), y: 270 + 220 * Math.sin(angle) },
      ];
    })
  );
  return (
    <section className="border-base-border border-t py-5" aria-label="Observed Fiber party pairs">
      <div className="mb-3 flex flex-wrap items-end justify-between gap-2">
        <div>
          <h2 className="text-text-bright text-[18px] font-medium">On-chain party pairs</h2>
          <p className="text-text-dim mt-1 text-[12px]">
            Lines join locks that opened a channel together. They do not show off-chain routes,
            payments, or live balances.
          </p>
          <p className="text-text-dim mt-1 text-[11px]">
            {representedChannels.toLocaleString()} of{' '}
            {network.summary.totalChannels.toLocaleString()} channel histories represented by pair
            lines. {unpairedChannels.toLocaleString()}{' '}
            {unpairedChannels === 1 ? 'channel has' : 'channels have'} a different number of
            distinct observed locks.
          </p>
        </div>
        <span className="text-text-dim font-mono text-[11px]">
          {network.summary.observedParties} observed locks · {network.summary.observedPairs} pairs
        </span>
      </div>
      {parties.length === 0 ? (
        <p className="text-text-dim py-10 text-[13px]">No on-chain parties observed yet.</p>
      ) : (
        <div className="grid gap-5 lg:grid-cols-[minmax(0,1fr)_260px]">
          <svg
            viewBox="0 0 860 540"
            role="img"
            aria-label={`On-chain graph of ${parties.length} locks and ${network.pairs.length} pairs`}
            className="bg-base-surface/20 border-base-border h-auto w-full border"
          >
            {network.pairs.map((pair) => {
              const first = positions.get(pair.firstLockScriptHash);
              const second = positions.get(pair.secondLockScriptHash);
              if (!first || !second)
                throw new Error(
                  `Fiber pair references unknown party ${pair.firstLockScriptHash} / ${pair.secondLockScriptHash}`
                );
              const active =
                selected?.firstLockScriptHash === pair.firstLockScriptHash &&
                selected.secondLockScriptHash === pair.secondLockScriptHash;
              return (
                <g
                  key={`${pair.firstLockScriptHash}:${pair.secondLockScriptHash}`}
                  role="button"
                  tabIndex={0}
                  aria-label={`Pair ${short(pair.firstLockScriptHash)} and ${short(pair.secondLockScriptHash)}, ${pair.channels} channels`}
                  onClick={() => setSelected(pair)}
                  onKeyDown={(event) => {
                    if (event.key === 'Enter' || event.key === ' ') {
                      event.preventDefault();
                      setSelected(pair);
                    }
                  }}
                  className="cursor-pointer"
                >
                  <line
                    x1={first.x}
                    y1={first.y}
                    x2={second.x}
                    y2={second.y}
                    stroke="transparent"
                    strokeWidth="16"
                  />
                  <line
                    x1={first.x}
                    y1={first.y}
                    x2={second.x}
                    y2={second.y}
                    stroke={active ? '#a0a8be' : pair.openChannels > 0 ? '#2edba3' : '#606b80'}
                    strokeWidth={Math.min(4, 1 + pair.channels / 3)}
                    opacity={active ? 1 : 0.65}
                  />
                </g>
              );
            })}
            {parties.map((party) => {
              const point = positions.get(party.lockScriptHash);
              if (!point) throw new Error(`Fiber graph missing ${party.lockScriptHash}`);
              return (
                <g key={party.lockScriptHash}>
                  <circle
                    cx={point.x}
                    cy={point.y}
                    r={Math.min(11, 4 + party.openChannels)}
                    fill={party.openChannels > 0 ? '#2edba3' : '#a0a8be'}
                    stroke="#10131c"
                    strokeWidth="2"
                  />
                  <title>{`${short(party.lockScriptHash)} · ${party.channels} observed channels`}</title>
                </g>
              );
            })}
          </svg>
          <div className="border-base-border min-w-0 border-l pl-4 text-[12px]">
            <div className="text-text-dim font-mono uppercase tracking-[0.12em]">
              {selected ? 'Selected pair' : 'Explore a pair'}
            </div>
            {selected ? (
              <div className="mt-3 space-y-3">
                {[selected.firstLockScriptHash, selected.secondLockScriptHash].map((hash) => {
                  const party = parties.find((item) => item.lockScriptHash === hash);
                  return (
                    <div key={hash} className="break-all font-mono text-[11px]">
                      {party?.address ? (
                        <Link
                          href={`/address/${party.address}`}
                          className="text-jade hover:underline"
                        >
                          {party.address}
                        </Link>
                      ) : (
                        <span className="text-text-dim">Unknown address · {hash}</span>
                      )}
                    </div>
                  );
                })}
                <div className="text-text-bright">
                  {selected.channels} channels ever · {selected.openChannels} open
                </div>
                <div className="text-text-dim">
                  {selected.cooperativelyClosedChannels} cooperative ·{' '}
                  {selected.forceClosedWaitingChannels} force-close waiting ·{' '}
                  {selected.forceClosedSettledChannels} settled
                </div>
                <div className="text-text-dim">
                  Open CKB capacity:{' '}
                  <span className="text-text-bright">
                    {formatCkbAmount(selected.openCapacity).full}
                  </span>
                </div>
                <div className="text-text-dim">
                  Opened in blocks{' '}
                  <Link href={`/blocks/${selected.firstOpenBlock}`} className="text-jade">
                    {formatBlockNumber(selected.firstOpenBlock)}
                  </Link>
                  –
                  <Link href={`/blocks/${selected.lastOpenBlock}`} className="text-jade">
                    {formatBlockNumber(selected.lastOpenBlock)}
                  </Link>
                </div>
              </div>
            ) : (
              <p className="text-text-dim mt-3 leading-relaxed">
                Select a line to inspect its exact channel counts and on-chain capacity. Address
                links appear only when the lock script is indexed.
              </p>
            )}
          </div>
        </div>
      )}
    </section>
  );
}
