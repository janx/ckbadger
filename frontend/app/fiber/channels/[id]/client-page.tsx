'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { FiberChannelBoard } from '@/components/fiber/fiber-channel-board';
import { api, isNotFoundApiError } from '@/lib/api';
import { FIBER_STATE_WORDS } from '@/lib/fiber-format';
import { NotFoundPage } from '@/components/not-found-page';
import { useParams } from '@/src/navigation';

export default function FiberChannelDetailPage() {
  const { id } = useParams();
  const channelId = String(id);
  const channel = useQuery({
    queryKey: ['fiber-channel', channelId],
    queryFn: () => api.getFiberChannel(channelId),
  });
  const evidence = useQuery({
    queryKey: ['fiber-channel-evidence', channelId],
    queryFn: () => api.getFiberChannelEvidence(channelId),
    enabled: Boolean(channel.data),
  });
  const udtTypeHash = channel.data?.udtTypeHash ?? null;
  const token = useQuery({
    queryKey: ['token', udtTypeHash],
    queryFn: () => api.getToken(udtTypeHash!),
    enabled: udtTypeHash !== null,
  });
  if (!channel.data && isNotFoundApiError(channel.error)) {
    return (
      <NotFoundPage
        entity={{
          kind: 'fiber-channel',
          identifier: channelId,
          checkedEndpoint: `/fiber/channels/${encodeURIComponent(channelId)}`,
        }}
      />
    );
  }
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1280px] px-4 pb-[72px] pt-7">
        <nav aria-label="Breadcrumb" className="text-text-dim text-[12px]">
          <Link href="/fiber/channels" className="hover:text-text">
            Fiber
          </Link>{' '}
          <span className="text-text-ghost">/</span> channels
        </nav>
        <div className="mt-2.5 flex flex-wrap items-baseline gap-3">
          <h1 className="text-text-bright text-[26px] font-medium">Fiber channel</h1>
          {channel.data && (
            <span className="text-text-dim text-[13px]">
              {FIBER_STATE_WORDS[channel.data.state]}
            </span>
          )}
        </div>
        <p className="text-text mt-2.5 break-all text-[13px]">{channelId}</p>
        {channel.isPending && (
          <p className="text-text-dim py-8 text-[13px]">Loading channel evidence…</p>
        )}
        {channel.isError && (
          <p role="alert" className="text-rouge py-8 text-[13px]">
            Channel could not be loaded: {String(channel.error)}
          </p>
        )}
        {token.isError && (
          <p role="alert" className="text-rouge mt-2 text-[12px]">
            Funding token could not be loaded: {String(token.error)}
          </p>
        )}
        {channel.data && (
          <div className="mt-2.5">
            <FiberChannelBoard
              channel={channel.data}
              evidence={evidence.data}
              evidenceError={evidence.isError ? String(evidence.error) : undefined}
              token={
                token.data
                  ? { symbol: token.data.symbol, decimals: token.data.decimals }
                  : undefined
              }
            />
          </div>
        )}
      </main>
    </div>
  );
}
