import SporeDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    sporeId: string;
  };
}

export default function Page({ params }: PageProps) {
  return <SporeDetailPage sporeId={params.sporeId} />;
}
