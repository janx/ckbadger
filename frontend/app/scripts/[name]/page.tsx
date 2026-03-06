import ScriptDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    name: string;
  };
}

export default function Page({ params }: PageProps) {
  return <ScriptDetailPage name={params.name} />;
}
