'use client';

import {
  IdentityNftItemDetail,
  type IdentityNftItemDetailConfig,
} from '@/components/nft/identity-nft-item-detail';
import { api } from '@/lib/api';

const dotbitConfig: IdentityNftItemDetailConfig = {
  standard: 'dotbit',
  fetchDetail: (nftId) => api.getDotbitItemDetail(nftId),
  fetchActivities: (nftId, params) => api.getDotbitItemActivities(nftId, params),
  labels: {
    standardDisplay: 'DOTBIT',
    nameLabel: '.bit Name',
    idLabel: 'Account ID',
    backLabel: 'Back to .bit Collection',
    backHref: '/nfts/dotbit',
    defaultTitle: '.bit account',
    notFoundMsg: '.bit item not found',
    recycledMsg: 'Recycled .bit account has no live cell.',
    showExpiry: true,
  },
};

export default function DotbitItemDetailPage() {
  return <IdentityNftItemDetail config={dotbitConfig} />;
}
