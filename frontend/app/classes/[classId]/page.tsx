import MnftClassDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    classId: string;
  };
}

export default function Page({ params }: PageProps) {
  return <MnftClassDetailPage classId={params.classId} />;
}
