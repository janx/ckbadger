'use client';

import { TopicCharts } from '@/components/charts/topic-charts';
import { useQuery } from '@tanstack/react-query';
import { Header } from '@/components/layout/header';
import { DaoBoard, DaoHeading, DaoLede } from '@/components/dao/dao-board';
import { api } from '@/lib/api';
import { useDocumentTitle } from '@/lib/use-document-title';

const DAO_TITLE = ['Nervos DAO'] as const;

export default function DaoPage() {
  useDocumentTitle(DAO_TITLE);
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

  // Hierarchy v3 (R13): the board opens with the title row and one short sentence; while the DAO
  // data loads the page shows the same title and sentence.
  const intro = (
    <div className="min-w-0">
      <DaoHeading />
      <DaoLede />
    </div>
  );

  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto px-4 pb-16 pt-6">
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
          />
        )}
        <TopicCharts topic="/dao" />
      </main>
    </div>
  );
}
