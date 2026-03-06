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
const CellAgeVsOccupiedCapacityPage = lazyPage(
  () => import('@/app/charts/cell-age-vs-occupied-capacity/page')
);
const CommonKnowledgeCompositionPage = lazyPage(
  () => import('@/app/charts/common-knowledge-composition/page')
);
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
  () => import('@/app/nfts/[sporeId]/client-page'),
  (params) => ({
    sporeId: params.sporeId ?? '',
  })
);
const MnftItemDetailRoute = lazyParamPage(
  () => import('@/app/nfts/mnft/[nftId]/client-page'),
  (params) => ({
    nftId: params.nftId ?? '',
  })
);
const DotbitItemDetailRoute = lazyParamPage(
  () => import('@/app/nfts/dotbit/[nftId]/client-page'),
  (params) => ({
    nftId: params.nftId ?? '',
  })
);
const DidCkbItemDetailRoute = lazyParamPage(
  () => import('@/app/nfts/did/[nftId]/client-page'),
  (params) => ({
    nftId: params.nftId ?? '',
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
          path: 'charts/cell-age-vs-occupied-capacity',
          element: <CellAgeVsOccupiedCapacityPage />,
        },
        {
          path: 'charts/common-knowledge-composition',
          element: <CommonKnowledgeCompositionPage />,
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
          path: 'nfts/:sporeId',
          element: <SporeDetailRoute />,
        },
        {
          path: 'nfts/mnft/:nftId',
          element: <MnftItemDetailRoute />,
        },
        {
          path: 'nfts/dotbit/:nftId',
          element: <DotbitItemDetailRoute />,
        },
        {
          path: 'nfts/did/:nftId',
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
