import FiberChannelsPage from './client-page';

export const revalidate = 0;
export const metadata = {
  title: 'Fiber Channels',
  description:
    'Fiber payment channels as the chain sees them: who opened channels with whom, what is locked now, and how each channel opened and closed.',
};

export default function Page() {
  return <FiberChannelsPage />;
}
