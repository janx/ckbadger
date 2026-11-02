'use client';

import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DaoBoard } from '@/components/dao/dao-board';
import { api } from '@/lib/api';

export default function DaoPage() {
  const statistics = useQuery({ queryKey: ['dao-statistics'], queryFn: api.getDaoStatistics });
  const evidence = useQuery({ queryKey: ['dao-evidence'], queryFn: api.getDaoEvidence });
  const deposited = useQuery({
    queryKey: ['dao-total-deposit-history'],
    queryFn: api.getDaoTotalDepositChart,
  });
  const ratio = useQuery({
    queryKey: ['dao-circulation-ratio-history'],
    queryFn: api.getDaoCirculationRatioChart,
  });

  // Hierarchy D1: one sentence and a pointer to the evidence; the concept itself is tier 2
  // ("How the Nervos DAO works"), and "Explore DAO charts →" sits beside History.
  const intro = (
    <div className="flex min-w-0 flex-col gap-2">
      <h1 className="text-text-bright text-index font-medium">Nervos DAO</h1>
      <p className="text-text text-body max-w-[1040px]">
        CKB locked in the Nervos DAO keeps its share of the supply while secondary issuance dilutes
        everything else.{' '}
        <a
          href="#evidence"
          className="text-text-dim hover:text-text-bright whitespace-nowrap hover:underline"
        >
          How it works ↓
        </a>
      </p>
    </div>
  );

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        {!(statistics.data && evidence.data) && <div className="mb-7">{intro}</div>}
        {statistics.isError && (
          <div role="alert" className="text-rouge text-body mb-4">
            DAO statistics could not be loaded: {String(statistics.error)}
          </div>
        )}
        {evidence.isError && (
          <div role="alert" className="text-rouge text-body mb-4">
            DAO chain evidence could not be loaded: {String(evidence.error)}
          </div>
        )}
        {!statistics.isError && !evidence.isError && (!statistics.data || !evidence.data) && (
          <div className="text-text-dim text-body py-8">Loading indexed DAO evidence…</div>
        )}
        {statistics.data && evidence.data && (
          <DaoBoard
            statistics={statistics.data}
            evidence={evidence.data}
            depositedHistory={deposited.data}
            circulationHistory={ratio.data}
            historyError={deposited.isError || ratio.isError}
            intro={intro}
          />
        )}
      </main>
    </div>
  );
}
