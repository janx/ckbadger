'use client';

import {
  IdentityItemDetail,
  type IdentityItemDetailConfig,
} from '@/components/identity/identity-item-detail';
import { api } from '@/lib/api';

const bitCellConfig: IdentityItemDetailConfig = {
  standard: 'bit_cell',
  fetchDetail: (identityId) => api.getBitCellItemDetail(identityId),
  fetchActivities: (identityId, params) => api.getBitCellItemActivities(identityId, params),
  labels: {
    standardDisplay: '.BIT CELL',
    nameLabel: '.bit Cell Name',
    idLabel: '.bit Cell ID',
    backLabel: 'Back to .bit Cell Collection',
    backHref: '/identities/bit-cell',
    defaultTitle: '.bit Cell identity',
    notFoundMsg: '.bit Cell identity not found',
    recycledMsg: 'Recycled .bit Cell identity has no live cell.',
    showExpiry: true,
    showVisualization: true,
  },
};

export interface BitCellItemDetailPageProps {
  identityId: string;
}

export default function BitCellItemDetailPage({ identityId }: BitCellItemDetailPageProps) {
  return <IdentityItemDetail config={bitCellConfig} identityId={identityId} />;
}
