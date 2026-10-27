'use client';

import Link from '@/components/ui/link';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DetailLayout, DetailSection } from '@/components/ui/detail-layout';
import { api } from '@/lib/api';
import { useActiveNetwork } from '@/hooks/useActiveNetwork';
import { ScriptDeploymentTimeline } from '@/components/hardforks/script-deployment-timeline';

export default function HardforksPage() {
  const network = useActiveNetwork();
  const { data, isLoading, error } = useQuery({
    queryKey: ['hardforks', network],
    queryFn: () => api.getHardforks({ network }),
    staleTime: 60_000,
  });
  const events = data?.events ?? [];
  const mirana = events.find((event) => event.id === 'mirana-2021');
  const meepo = events.find((event) => event.id === 'meepo-2024');
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <DetailLayout
          title="Hardforks"
          kind="topic"
          identity="CKB rules change at consensus activation epochs. A script's hash type determines which VM version runs it."
          status={
            data && (
              <span className="text-text-dim text-xs">
                {data.network} · tip epoch {data.tipEpoch.toLocaleString()}
              </span>
            )
          }
        >
          {isLoading && <p className="text-text-dim">Loading hardfork timeline…</p>}
          {error && (
            <p role="alert" className="text-negative">
              Failed to load hardfork timeline: {String(error)}
            </p>
          )}
          {data && (!mirana || !meepo) && (
            <p role="alert" className="text-negative">
              Consensus edition timeline is incomplete.
            </p>
          )}
          {data && mirana && meepo && (
            <>
              <DetailSection title="Which VM runs a script, over time">
                <p className="text-text-dim mb-5 max-w-4xl text-[13px] leading-relaxed">
                  A type-hash script follows consensus upgrades. A data-hash script stays pinned to
                  the VM selected by its hash type.
                </p>
                <div
                  className="border-base-border-subtle min-w-0 border-y py-4"
                  role="table"
                  aria-label="CKB VM version by hash type and hardfork era"
                >
                  <div
                    role="row"
                    className="text-text-dim grid grid-cols-[7rem_repeat(3,minmax(0,1fr))] gap-2 text-[11px] sm:grid-cols-[10rem_repeat(3,minmax(0,1fr))]"
                  >
                    <span role="columnheader">Hash type</span>
                    <span role="columnheader">Genesis</span>
                    <span role="columnheader">
                      Mirana · epoch {mirana.activationEpoch.toLocaleString()}
                    </span>
                    <span role="columnheader">
                      Meepo · epoch {meepo.activationEpoch.toLocaleString()}
                    </span>
                  </div>
                  {(
                    [
                      ['type', 'VM0', 'VM1', 'VM2'],
                      ['data', 'VM0', 'VM0', 'VM0'],
                      ['data1', '—', 'VM1', 'VM1'],
                      ['data2', '—', '—', 'VM2'],
                    ] as const
                  ).map(([hashType, genesis, first, second]) => (
                    <div
                      role="row"
                      key={hashType}
                      className="border-base-border-subtle grid grid-cols-[7rem_repeat(3,minmax(0,1fr))] gap-2 border-t py-3 text-[12px] sm:grid-cols-[10rem_repeat(3,minmax(0,1fr))]"
                    >
                      <span role="rowheader" className="text-text-bright break-all">
                        {hashType}
                      </span>
                      {[genesis, first, second].map((version, index) => (
                        <span
                          role="cell"
                          key={index}
                          className={version === '—' ? 'text-text-dim' : 'text-jade'}
                        >
                          {version}
                        </span>
                      ))}
                    </div>
                  ))}
                </div>
                <div className="border-base-border-subtle mt-7 border-t pt-4">
                  <h3 className="text-text-bright mb-2 text-[13px]">
                    Known code-version deployments
                  </h3>
                  <p className="text-text-dim mb-4 text-[11px]">
                    Dots mark the earliest indexed code-cell creation for each named script version.
                    Hover or focus a mark for its exact block, raw header time, epoch and cell
                    evidence.
                  </p>
                  <ScriptDeploymentTimeline deployments={data.scriptDeployments} />
                </div>
              </DetailSection>
              <DetailSection title="Editions and activation boundaries">
                <div className="grid gap-6 md:grid-cols-2">
                  {events.map((event) => (
                    <article
                      key={event.id}
                      className="border-base-border-subtle min-w-0 border-t pt-4"
                    >
                      <div className="flex flex-wrap items-baseline justify-between gap-2">
                        <h3 className="text-text-bright text-[15px]">{event.name}</h3>
                        <span className="text-jade text-[11px]">{event.status}</span>
                      </div>
                      <p className="text-text-dim mt-1 text-[12px]">
                        Epoch {event.activationEpoch.toLocaleString()} · {event.activationDate}
                      </p>
                      <p className="text-text mt-3 text-[13px]">{event.summary}</p>
                      <div className="mt-4 flex flex-wrap gap-x-5 gap-y-2 text-[12px]">
                        {event.activationBlock !== null && (
                          <Link
                            href={'/blocks/' + event.activationBlock}
                            className="text-emphasis hover:underline"
                          >
                            View activation block #{event.activationBlock.toLocaleString()}
                          </Link>
                        )}
                        {event.resources.map((resource) => (
                          <a
                            key={resource.url}
                            href={resource.url}
                            target="_blank"
                            rel="noreferrer"
                            className="text-emphasis hover:underline"
                          >
                            {resource.label}
                          </a>
                        ))}
                      </div>
                    </article>
                  ))}
                </div>
              </DetailSection>
              <DetailSection title="As this node's consensus states it">
                <p className="text-text-dim mb-4 text-[12px]">
                  get_consensus · hardfork_features · {data.network}. Epoch 0 features were active
                  from genesis.
                </p>
                <dl className="grid grid-cols-2 gap-x-8 gap-y-3 sm:grid-cols-3 lg:grid-cols-5">
                  {data.consensusFeatures.map((feature) => (
                    <div
                      key={feature.rfc}
                      className="border-base-border-subtle border-t pt-2 text-[12px]"
                    >
                      <dt className="text-text-bright">RFC {feature.rfc}</dt>
                      <dd className="text-text-dim">
                        {feature.activationEpoch === null
                          ? 'not activated'
                          : 'epoch ' + feature.activationEpoch.toLocaleString()}
                      </dd>
                    </div>
                  ))}
                </dl>
              </DetailSection>
            </>
          )}
        </DetailLayout>
      </main>
    </div>
  );
}
