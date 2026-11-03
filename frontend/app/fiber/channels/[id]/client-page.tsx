'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { FiberChannelBoard } from '@/components/fiber/fiber-channel-board';
import { api, isNotFoundApiError } from '@/lib/api';
import { NotFoundPage } from '@/components/not-found-page';
import { useParams } from '@/src/navigation';
import { DocumentTitle, titleHash } from '@/lib/use-document-title';

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
    <div className="min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1280px] pb-[72px] pt-7">
        <DocumentTitle
          parts={
            channel.data ? [titleHash(channel.data.channelId), 'Fiber channel'] : ['Fiber channel']
          }
        />
        <nav aria-label="Breadcrumb" className="text-text-dim text-small">
          <Link href="/fiber/channels" className="hover:text-text">
            Fiber
          </Link>{' '}
          <span className="text-text-ghost">/</span> channels
        </nav>
        {/* Hierarchy v3: once loaded, the board's hero section holds the title, state and id. */}
        {!channel.data && (
          <>
            <h1 className="text-text-bright text-entity mt-2.5 font-medium">Fiber channel</h1>
            <p className="text-text-dim text-small mt-1.5 break-all">{channelId}</p>
          </>
        )}
        {channel.isPending && (
          <p className="text-text-dim text-body py-8">Loading channel evidence…</p>
        )}
        {channel.isError && (
          <p role="alert" className="text-rouge text-body py-8">
            Channel could not be loaded: {String(channel.error)}
          </p>
        )}
        {token.isError && (
          <p role="alert" className="text-rouge text-small mt-2">
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
