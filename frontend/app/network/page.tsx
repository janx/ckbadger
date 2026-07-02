import { NetworkClientPage } from './client-page';

export const revalidate = 0;
export const metadata = {
  title: 'Peers',
  description:
    'Whole-network CKB L1 peer discovery — the discoverable, reachable nodes this crawler can see.',
};

export default function Page() {
  return <NetworkClientPage />;
}
