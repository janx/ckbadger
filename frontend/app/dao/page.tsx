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

  const intro = (
    <div className="flex min-w-0 flex-col gap-2.5">
      <h1 className="text-text-bright text-[26px] font-medium">Nervos DAO</h1>
      <p className="text-text max-w-[700px] text-[15px] leading-relaxed">
        CKB locked in the Nervos DAO keeps its share of the supply. Secondary issuance, 1.344
        billion CKB a year, is split by how CKB is used: depositors are compensated with the share
        their deposits make up, miners get the share that is occupied, and the rest is burnt.
      </p>
      <Link
        href="/charts/total-deposit"
        className="text-text hover:text-text-bright w-fit font-mono text-[12px] hover:underline"
      >
        Explore DAO charts →
      </Link>
    </div>
  );

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        {!(statistics.data && evidence.data) && <div className="mb-7">{intro}</div>}
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
            intro={intro}
          />
        )}
      </main>
    </div>
  );
}
