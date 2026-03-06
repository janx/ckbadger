import type { RouteObject } from 'react-router-dom';
import { Outlet, useParams } from 'react-router-dom';
import AddressDetailPage from '@/app/address/[addr]/page';
import { SiteFooter } from '@/components/layout/site-footer';
import AssetsPage from '@/app/assets/page';
import BlockDetailPage from '@/app/blocks/[id]/page';
import BlocksPage from '@/app/blocks/page';
import CapacityTurnoverRatioPage from '@/app/charts/capacity-turnover-ratio/page';
import CellAgeVsOccupiedCapacityPage from '@/app/charts/cell-age-vs-occupied-capacity/page';
import ChartsPage from '@/app/charts/page';
import CommonKnowledgeCompositionPage from '@/app/charts/common-knowledge-composition/page';
import EpochTimeLengthPage from '@/app/charts/epoch-time-length/page';
import HodlWavePage from '@/app/charts/hodl-wave/page';
import KnowledgeSizePage from '@/app/charts/knowledge-size/page';
import MostUtilizedAssetsPage from '@/app/charts/most-utilized-assets/page';
import MostUtilizedScriptsPage from '@/app/charts/most-utilized-scripts/page';
import SecondaryIssuancePage from '@/app/charts/secondary-issuance/page';
import TotalSupplyPage from '@/app/charts/total-supply/page';
import CellDetailPage from '@/app/cell/[outpoint]/page';
import Home from '@/app/page';
import DaoPage from '@/app/dao/page';
import ForkDetailPage from '@/app/forks/[id]/page';
import ForksPage from '@/app/forks/page';
import HardforksPage from '@/app/hardforks/page';
import ScriptByCodeHashPage from '@/app/script/[codeHash]/client-page';
import ScriptDetailPage from '@/app/scripts/[name]/client-page';
import ScriptsPage from '@/app/scripts/page';
import TokenDetailPage from '@/app/tokens/[typeHash]/client-page';
import ClusterDetailPage from '@/app/clusters/[clusterId]/client-page';
import SporeDetailPage from '@/app/nfts/[sporeId]/client-page';
import MnftItemDetailPage from '@/app/nfts/mnft/[nftId]/client-page';
import DotbitItemDetailPage from '@/app/nfts/dotbit/[nftId]/client-page';
import DidCkbItemDetailPage from '@/app/nfts/did/[nftId]/client-page';
import TransactionsPage from '@/app/transactions/page';
import TransactionDetailPage from '@/app/tx/[hash]/page';
import { NotFoundPage } from '@/components/not-found-page';

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

function ScriptByCodeHashRoute() {
  const { codeHash = '' } = useParams();
  return <ScriptByCodeHashPage codeHash={codeHash} />;
}

function ScriptDetailRoute() {
  const { name = '' } = useParams();
  return <ScriptDetailPage name={name} />;
}

function TokenDetailRoute() {
  const { typeHash = '' } = useParams();
  return <TokenDetailPage typeHash={typeHash} />;
}

function ClusterDetailRoute() {
  const { clusterId = '' } = useParams();
  return <ClusterDetailPage clusterId={clusterId} />;
}

function SporeDetailRoute() {
  const { sporeId = '' } = useParams();
  return <SporeDetailPage sporeId={sporeId} />;
}

function MnftItemDetailRoute() {
  const { nftId = '' } = useParams();
  return <MnftItemDetailPage nftId={nftId} />;
}

function DotbitItemDetailRoute() {
  const { nftId = '' } = useParams();
  return <DotbitItemDetailPage nftId={nftId} />;
}

function DidCkbItemDetailRoute() {
  const { nftId = '' } = useParams();
  return <DidCkbItemDetailPage nftId={nftId} />;
}

export function createAppRouter(): RouteObject[] {
  return [
    {
      path: '/',
      element: <AppFrame />,
      children: [
        {
          index: true,
          element: <Home />,
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
