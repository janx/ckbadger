import ClusterDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    clusterId: string;
  };
}

export default function Page({ params }: PageProps) {
  return <ClusterDetailPage clusterId={params.clusterId} />;
}
