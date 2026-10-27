'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
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

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <div className="mb-7 flex flex-wrap items-start justify-between gap-4">
          <div>
            <div className="font-mono text-[11px] uppercase tracking-[0.16em] text-[#f2c55c]">
              CKB economics
            </div>
            <h1 className="text-text-bright mt-1 text-[26px] font-semibold">Nervos DAO</h1>
            <p className="text-text-dim mt-2 max-w-4xl text-[13px]">
              CKB locked in Nervos DAO keeps its share of supply. Secondary issuance is allocated
              between miners, compensation for depositors, and the burnt remainder.
            </p>
          </div>
          <Link
            href="/charts/total-deposit"
            className="border border-[#f2c55c]/30 px-3 py-1.5 font-mono text-[12px] text-[#f2c55c] hover:bg-[#f2c55c]/10"
          >
            Explore DAO charts →
          </Link>
        </div>
        {statistics.isError && (
          <div role="alert" className="text-rouge mb-4 text-[13px]">
            DAO statistics could not be loaded: {String(statistics.error)}
          </div>
        )}
        {evidence.isError && (
          <div role="alert" className="text-rouge mb-4 text-[13px]">
            DAO chain evidence could not be loaded: {String(evidence.error)}
          </div>
        )}
        {!statistics.isError && !evidence.isError && (!statistics.data || !evidence.data) && (
          <div className="text-text-dim py-8 text-[13px]">Loading indexed DAO evidence…</div>
        )}
        {statistics.data && evidence.data && (
          <DaoBoard
            statistics={statistics.data}
            evidence={evidence.data}
            depositedHistory={deposited.data}
            circulationHistory={ratio.data}
            historyError={deposited.isError || ratio.isError}
          />
        )}
      </main>
    </div>
  );
}
