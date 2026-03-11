'use client';

import {
  IdentityItemDetail,
  type IdentityItemDetailConfig,
} from '@/components/identity/identity-item-detail';
import { api } from '@/lib/api';

const didCkbConfig: IdentityItemDetailConfig = {
  standard: 'did_ckb',
  fetchDetail: (nftId) => api.getDidCkbItemDetail(nftId),
  fetchActivities: (nftId, params) => api.getDidCkbItemActivities(nftId, params),
  labels: {
    standardDisplay: 'DID:CKB',
    nameLabel: 'did:ckb Name',
    idLabel: 'DID ID',
    backLabel: 'Back to did:ckb Collection',
    backHref: '/identities/did:ckb',
    defaultTitle: 'did:ckb identity',
    notFoundMsg: 'did:ckb item not found',
    recycledMsg: 'Recycled did:ckb identity has no live cell.',
    showExpiry: false,
  },
};

export interface DidCkbItemDetailPageProps {
  identityId: string;
}

export default function DidCkbItemDetailPage({ identityId }: DidCkbItemDetailPageProps) {
  return <IdentityItemDetail config={didCkbConfig} nftId={identityId} />;
}
