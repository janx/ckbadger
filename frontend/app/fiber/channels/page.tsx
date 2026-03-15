import FiberChannelsPage from './client-page';

export const revalidate = 0;
export const metadata = {
  title: 'Fiber Channels',
  description:
    'Follow the living circuitry of Fiber on Nervos, where nodes whisper value through channels like signals across a sleepless mind.',
};

export default function Page() {
  return <FiberChannelsPage />;
}
