import ScriptByCodeHashPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

interface PageProps {
  params: {
    codeHash: string;
  };
}

export default function Page({ params }: PageProps) {
  return <ScriptByCodeHashPage codeHash={params.codeHash} />;
}
