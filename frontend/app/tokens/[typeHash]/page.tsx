import TokenDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    typeHash: string;
  };
}

export default function Page({ params }: PageProps) {
  return <TokenDetailPage typeHash={params.typeHash} />;
}
