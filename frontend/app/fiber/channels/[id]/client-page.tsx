'use client';

import { useQuery } from '@tanstack/react-query';
import Link from '@/components/ui/link';
import { Header } from '@/components/layout/header';
import { FiberChannelBoard } from '@/components/fiber/fiber-channel-board';
import { api } from '@/lib/api';
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
  return (
    <div className="bg-base-bg min-h-screen">
      <Header />
      <main className="container mx-auto max-w-[1440px] px-4 pb-16 pt-6">
        <div className="text-jade font-mono text-[11px] uppercase tracking-[0.16em]">
          <Link href="/fiber/channels" className="hover:underline">
            Fiber channels
          </Link>{' '}
          / on-chain channel
        </div>
        <h1 className="text-text-bright mt-1 text-[26px] font-semibold">Fiber channel</h1>
        <p className="text-text-dim mt-2 break-all font-mono text-[12px]">{channelId}</p>
        {channel.isPending && (
          <p className="text-text-dim py-8 text-[13px]">Loading channel evidence…</p>
        )}
        {channel.isError && (
          <p role="alert" className="text-rouge py-8 text-[13px]">
            Channel could not be loaded: {String(channel.error)}
          </p>
        )}
        {channel.data && (
          <div className="mt-6">
            <FiberChannelBoard
              channel={channel.data}
              evidence={evidence.data}
              evidenceError={evidence.isError ? String(evidence.error) : undefined}
            />
          </div>
        )}
      </main>
    </div>
  );
}
