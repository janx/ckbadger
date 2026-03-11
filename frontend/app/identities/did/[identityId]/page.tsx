import DidCkbItemDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    identityId: string;
  };
}

export default function Page({ params }: PageProps) {
  return <DidCkbItemDetailPage identityId={params.identityId} />;
}
