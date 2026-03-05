import CellDetailPage from './client-page';

export const revalidate = 0;

export async function generateStaticParams() {
  return [];
}

export default function Page() {
  return <CellDetailPage />;
}
