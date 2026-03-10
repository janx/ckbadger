import IdentityCollectionPage from './client-page';

interface Props {
  params: Promise<{ collectionId: string }>;
}

export default async function Page({ params }: Props) {
  const { collectionId } = await params;
  return <IdentityCollectionPage collectionId={collectionId} />;
}
