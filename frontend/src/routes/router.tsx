import type { ComponentType } from 'react';
import type { RouteObject } from 'react-router-dom';
import { Outlet, useParams } from 'react-router-dom';
import { SiteFooter } from '@/components/layout/site-footer';
import { NotFoundPage } from '@/components/not-found-page';
import dynamic from '@/lib/dynamic-client';

type PageModule<TProps extends object = object> = {
  default: ComponentType<TProps>;
};

type ParamRecord = Record<string, string | undefined>;

function AppFrame() {
  return (
    <div className="flex min-h-screen flex-col">
      <div className="flex-1">
        <Outlet />
      </div>
      <SiteFooter />
    </div>
  );
}

function lazyPage<TProps extends object>(loader: () => Promise<PageModule<TProps>>) {
  return dynamic(loader, {
    loading: () => <div className="min-h-[240px]" />,
  });
}

function lazyParamPage<TProps extends object>(
  loader: () => Promise<PageModule<TProps>>,
  mapParams: (params: ParamRecord) => TProps
) {
  const LazyPage = dynamic(loader, {
    loading: () => <div className="min-h-[240px]" />,
  });

  return function ParamPage() {
    return <LazyPage {...mapParams(useParams())} />;
  };
}

const HomePage = lazyPage(() => import('@/app/page'));
const AssetsPage = lazyPage(() => import('@/app/assets/page'));
const BlocksPage = lazyPage(() => import('@/app/blocks/page'));
const BlockDetailPage = lazyPage(() => import('@/app/blocks/[id]/page'));
const TransactionsPage = lazyPage(() => import('@/app/transactions/page'));
const TransactionDetailPage = lazyPage(() => import('@/app/tx/[hash]/page'));
const AddressDetailPage = lazyPage(() => import('@/app/address/[addr]/page'));
const CellDetailPage = lazyPage(() => import('@/app/cell/[outpoint]/page'));
const ScriptsPage = lazyPage(() => import('@/app/scripts/page'));
const ForksPage = lazyPage(() => import('@/app/forks/page'));
const ForkDetailPage = lazyPage(() => import('@/app/forks/[id]/page'));
const ChartsPage = lazyPage(() => import('@/app/charts/page'));
const MostUtilizedScriptsPage = lazyPage(() => import('@/app/charts/most-utilized-scripts/page'));
const MostUtilizedAssetsPage = lazyPage(() => import('@/app/charts/most-utilized-assets/page'));
const SecondaryIssuancePage = lazyPage(() => import('@/app/charts/secondary-issuance/page'));
const TotalSupplyPage = lazyPage(() => import('@/app/charts/total-supply/page'));
const KnowledgeSizePage = lazyPage(() => import('@/app/charts/knowledge-size/page'));
const HodlWavePage = lazyPage(() => import('@/app/charts/hodl-wave/page'));
const EpochTimeLengthPage = lazyPage(() => import('@/app/charts/epoch-time-length/page'));
const CapacityTurnoverRatioPage = lazyPage(
  () => import('@/app/charts/capacity-turnover-ratio/page')
);
const CellAgeVsUsedCapacityPage = lazyPage(
  () => import('@/app/charts/cell-age-vs-used-capacity/page')
);
const CommonKnowledgeCompositionPage = lazyPage(
  () => import('@/app/charts/common-knowledge-composition/page')
);
const ActivityVolumePage = lazyPage(() => import('@/app/charts/activity-volume/page'));
const ActivityTypeBreakdownPage = lazyPage(
  () => import('@/app/charts/activity-type-breakdown/page')
);
const ActiveAddressesPage = lazyPage(() => import('@/app/charts/active-addresses/page'));
const CkbVolumePage = lazyPage(() => import('@/app/charts/ckb-volume/page'));
const AddressCohortRetentionPage = lazyPage(
  () => import('@/app/charts/address-cohort-retention/page')
);
const AverageBlockTimePage = lazyPage(() => import('@/app/charts/average-block-time/page'));
const BlockTimeDistributionPage = lazyPage(
  () => import('@/app/charts/block-time-distribution/page')
);
const CellCountPage = lazyPage(() => import('@/app/charts/cell-count/page'));
const CellSizeDistributionPage = lazyPage(() => import('@/app/charts/cell-size-distribution/page'));
const CirculationRatioPage = lazyPage(() => import('@/app/charts/circulation-ratio/page'));
const DailyDepositPage = lazyPage(() => import('@/app/charts/daily-deposit/page'));
const DifficultyPage = lazyPage(() => import('@/app/charts/difficulty/page'));
const EpochTimeDistributionPage = lazyPage(
  () => import('@/app/charts/epoch-time-distribution/page')
);
const HashRatePage = lazyPage(() => import('@/app/charts/hash-rate/page'));
const InflationRatePage = lazyPage(() => import('@/app/charts/inflation-rate/page'));
const MinerAddressDistributionPage = lazyPage(
  () => import('@/app/charts/miner-address-distribution/page')
);
const NominalApcPage = lazyPage(() => import('@/app/charts/nominal-apc/page'));
const TotalDepositPage = lazyPage(() => import('@/app/charts/total-deposit/page'));
const TransactionCountPage = lazyPage(() => import('@/app/charts/transaction-count/page'));
const UncleRatePage = lazyPage(() => import('@/app/charts/uncle-rate/page'));
const DaoPage = lazyPage(() => import('@/app/dao/page'));
const HardforksPage = lazyPage(() => import('@/app/hardforks/page'));
const ScriptByCodeHashRoute = lazyParamPage(
  () => import('@/app/script/[codeHash]/client-page'),
  (params) => ({
    codeHash: params.codeHash ?? '',
  })
);
const ScriptDetailRoute = lazyParamPage(
  () => import('@/app/scripts/[name]/client-page'),
  (params) => ({
    name: params.name ?? '',
  })
);
const TokenDetailRoute = lazyParamPage(
  () => import('@/app/tokens/[typeHash]/client-page'),
  (params) => ({
    typeHash: params.typeHash ?? '',
  })
);
const ClusterDetailRoute = lazyParamPage(
  () => import('@/app/clusters/[clusterId]/client-page'),
  (params) => ({
    clusterId: params.clusterId ?? '',
  })
);
const SporeDetailRoute = lazyParamPage(
  () => import('@/app/objects/[sporeId]/client-page'),
  (params) => ({
    sporeId: params.sporeId ?? '',
  })
);
const MnftItemDetailRoute = lazyParamPage(
  () => import('@/app/objects/mnft/[objectId]/client-page'),
  (params) => ({
    objectId: params.objectId ?? '',
  })
);
const DotbitItemDetailRoute = lazyParamPage(
  () => import('@/app/identities/dotbit/[identityId]/client-page'),
  (params) => ({
    identityId: params.identityId ?? '',
  })
);
const DidCkbItemDetailRoute = lazyParamPage(
  () => import('@/app/identities/did/[identityId]/client-page'),
  (params) => ({
    identityId: params.identityId ?? '',
  })
);
const IdentityCollectionRoute = lazyParamPage(
  () => import('@/app/identities/[collectionId]/client-page'),
  (params) => ({
    collectionId: params.collectionId ?? '',
  })
);

export function createAppRouter(): RouteObject[] {
  return [
    {
      path: '/',
      element: <AppFrame />,
      children: [
        {
          index: true,
          element: <HomePage />,
        },
        {
          path: 'assets',
          element: <AssetsPage />,
        },
        {
          path: 'blocks',
          element: <BlocksPage />,
        },
        {
          path: 'blocks/:id',
          element: <BlockDetailPage />,
        },
        {
          path: 'transactions',
          element: <TransactionsPage />,
        },
        {
          path: 'tx/:hash',
          element: <TransactionDetailPage />,
        },
        {
          path: 'address/:addr',
          element: <AddressDetailPage />,
        },
        {
          path: 'cell/:outpoint',
          element: <CellDetailPage />,
        },
        {
          path: 'scripts',
          element: <ScriptsPage />,
        },
        {
          path: 'forks',
          element: <ForksPage />,
        },
        {
          path: 'forks/:id',
          element: <ForkDetailPage />,
        },
        {
          path: 'charts',
          element: <ChartsPage />,
        },
        {
          path: 'charts/most-utilized-scripts',
          element: <MostUtilizedScriptsPage />,
        },
        {
          path: 'charts/most-utilized-assets',
          element: <MostUtilizedAssetsPage />,
        },
        {
          path: 'charts/secondary-issuance',
          element: <SecondaryIssuancePage />,
        },
        {
          path: 'charts/total-supply',
          element: <TotalSupplyPage />,
        },
        {
          path: 'charts/knowledge-size',
          element: <KnowledgeSizePage />,
        },
        {
          path: 'charts/hodl-wave',
          element: <HodlWavePage />,
        },
        {
          path: 'charts/epoch-time-length',
          element: <EpochTimeLengthPage />,
        },
        {
          path: 'charts/capacity-turnover-ratio',
          element: <CapacityTurnoverRatioPage />,
        },
        {
          path: 'charts/cell-age-vs-used-capacity',
          element: <CellAgeVsUsedCapacityPage />,
        },
        {
          path: 'charts/common-knowledge-composition',
          element: <CommonKnowledgeCompositionPage />,
        },
        {
          path: 'charts/activity-volume',
          element: <ActivityVolumePage />,
        },
        {
          path: 'charts/activity-type-breakdown',
          element: <ActivityTypeBreakdownPage />,
        },
        {
          path: 'charts/active-addresses',
          element: <ActiveAddressesPage />,
        },
        {
          path: 'charts/ckb-volume',
          element: <CkbVolumePage />,
        },
        {
          path: 'charts/address-cohort-retention',
          element: <AddressCohortRetentionPage />,
        },
        {
          path: 'charts/average-block-time',
          element: <AverageBlockTimePage />,
        },
        {
          path: 'charts/block-time-distribution',
          element: <BlockTimeDistributionPage />,
        },
        {
          path: 'charts/cell-count',
          element: <CellCountPage />,
        },
        {
          path: 'charts/cell-size-distribution',
          element: <CellSizeDistributionPage />,
        },
        {
          path: 'charts/circulation-ratio',
          element: <CirculationRatioPage />,
        },
        {
          path: 'charts/daily-deposit',
          element: <DailyDepositPage />,
        },
        {
          path: 'charts/difficulty',
          element: <DifficultyPage />,
        },
        {
          path: 'charts/epoch-time-distribution',
          element: <EpochTimeDistributionPage />,
        },
        {
          path: 'charts/hash-rate',
          element: <HashRatePage />,
        },
        {
          path: 'charts/inflation-rate',
          element: <InflationRatePage />,
        },
        {
          path: 'charts/miner-address-distribution',
          element: <MinerAddressDistributionPage />,
        },
        {
          path: 'charts/nominal-apc',
          element: <NominalApcPage />,
        },
        {
          path: 'charts/total-deposit',
          element: <TotalDepositPage />,
        },
        {
          path: 'charts/transaction-count',
          element: <TransactionCountPage />,
        },
        {
          path: 'charts/uncle-rate',
          element: <UncleRatePage />,
        },
        {
          path: 'dao',
          element: <DaoPage />,
        },
        {
          path: 'hardforks',
          element: <HardforksPage />,
        },
        {
          path: 'script/:codeHash',
          element: <ScriptByCodeHashRoute />,
        },
        {
          path: 'scripts/:name',
          element: <ScriptDetailRoute />,
        },
        {
          path: 'tokens/:typeHash',
          element: <TokenDetailRoute />,
        },
        {
          path: 'clusters/:clusterId',
          element: <ClusterDetailRoute />,
        },
        {
          path: 'identities/:collectionId',
          element: <IdentityCollectionRoute />,
        },
        {
          path: 'objects/:sporeId',
          element: <SporeDetailRoute />,
        },
        {
          path: 'objects/mnft/:objectId',
          element: <MnftItemDetailRoute />,
        },
        {
          path: 'identities/dotbit/:identityId',
          element: <DotbitItemDetailRoute />,
        },
        {
          path: 'identities/did/:identityId',
          element: <DidCkbItemDetailRoute />,
        },
        {
          path: '*',
          element: <NotFoundPage />,
        },
      ],
    },
  ];
}
