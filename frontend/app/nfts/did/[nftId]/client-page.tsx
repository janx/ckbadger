'use client';

import {
  IdentityNftItemDetail,
  type IdentityNftItemDetailConfig,
} from '@/components/nft/identity-nft-item-detail';
import { api } from '@/lib/api';

const didCkbConfig: IdentityNftItemDetailConfig = {
  standard: 'did_ckb',
  fetchDetail: (nftId) => api.getDidCkbItemDetail(nftId),
  fetchActivities: (nftId, params) => api.getDidCkbItemActivities(nftId, params),
  labels: {
    standardDisplay: 'DID:CKB',
    nameLabel: 'did:ckb Name',
    idLabel: 'DID ID',
    backLabel: 'Back to did:ckb Collection',
    backHref: '/nfts/did:ckb',
    defaultTitle: 'did:ckb identity',
    notFoundMsg: 'did:ckb item not found',
    recycledMsg: 'Recycled did:ckb identity has no live cell.',
    showExpiry: false,
  },
};

export interface DidCkbItemDetailPageProps {
  nftId: string;
}

export default function DidCkbItemDetailPage({ nftId }: DidCkbItemDetailPageProps) {
  return <IdentityNftItemDetail config={didCkbConfig} nftId={nftId} />;
}
