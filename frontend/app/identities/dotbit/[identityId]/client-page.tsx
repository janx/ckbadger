'use client';

import {
  IdentityItemDetail,
  type IdentityItemDetailConfig,
} from '@/components/identity/identity-item-detail';
import { api } from '@/lib/api';

const dotbitConfig: IdentityItemDetailConfig = {
  standard: 'dotbit',
  fetchDetail: (nftId) => api.getDotbitItemDetail(nftId),
  fetchActivities: (nftId, params) => api.getDotbitItemActivities(nftId, params),
  labels: {
    standardDisplay: 'DOTBIT',
    nameLabel: '.bit Name',
    idLabel: 'Account ID',
    backLabel: 'Back to .bit Collection',
    backHref: '/identities/dotbit',
    defaultTitle: '.bit account',
    notFoundMsg: '.bit item not found',
    recycledMsg: 'Recycled .bit account has no live cell.',
    showExpiry: true,
  },
};

export interface DotbitItemDetailPageProps {
  identityId: string;
}

export default function DotbitItemDetailPage({ identityId }: DotbitItemDetailPageProps) {
  return <IdentityItemDetail config={dotbitConfig} nftId={identityId} />;
}
