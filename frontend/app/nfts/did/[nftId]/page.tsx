import DidCkbItemDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    nftId: string;
  };
}

export default function Page({ params }: PageProps) {
  return <DidCkbItemDetailPage nftId={params.nftId} />;
}
