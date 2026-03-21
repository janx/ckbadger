import { Tooltip } from '@/components/spore/cluster-description';

const STORAGE_TIER_DESCRIPTIONS: Record<string, string> = {
  fully_on_ckb:
    'All content is stored directly on the CKB blockchain (on-chain data or ckbfs://). Fully verifiable and permanent.',
  fully_on_ckb_and_btc:
    'Content is stored across both CKB (on-chain data or ckbfs://) and Bitcoin (btcfs://). Fully verifiable and permanent.',
  decentralized_dependent:
    'Some content references external decentralized storage (e.g. IPFS, Arweave). Data persists as long as the external network hosts it.',
  centralized_dependent:
    'Some content depends on centralized servers (http/https). Data availability relies on the server operator.',
  unknown:
    'Storage profile could not be determined. The content storage method for objects in this cluster is unverified.',
};

const TOOLTIP_BTN_BASE =
  'ml-1 inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border font-mono text-[9px] leading-none transition-colors';

export function storageTierCardStyle(tier: string): {
  card: string;
  label: string;
  text: string;
  tooltipButton?: string;
} {
  if (tier === 'fully_on_ckb_and_btc') {
    return {
      card: 'storage-card-no-crt storage-card-both rounded border border-[#4a6838] bg-gradient-to-br from-[#4a3a12] to-[#0e3830] p-3',
      label: 'text-[#a0b880]',
      text: 'storage-text-split',
      tooltipButton: `${TOOLTIP_BTN_BASE} text-[#a0b880] border-[#4a6838] hover:text-[#c0d8a0] hover:border-[#6a8850]`,
    };
  }
  if (tier === 'fully_on_ckb' || tier === 'fully_onchain') {
    return {
      card: 'storage-card-no-crt storage-card-ckb rounded border border-[#1a6050] bg-[#0e3830] p-3',
      label: 'text-[#5abfa0]',
      text: 'storage-text-gem',
      tooltipButton: `${TOOLTIP_BTN_BASE} text-[#5abfa0] border-[#1a6050] hover:text-[#40e8b0] hover:border-[#2a8068]`,
    };
  }
  if (tier === 'centralized_dependent') {
    return {
      card: 'border-base-border rounded border p-3',
      label: 'text-text-dim',
      text: 'text-negative',
    };
  }
  return {
    card: 'border-base-border rounded border p-3',
    label: 'text-text-dim',
    text: 'text-warning',
  };
}

/**
 * Returns panel-level CSS classes for the content preview TerminalPanel,
 * applying the storage tier visual language to the panel border and header.
 */
export function previewPanelStyle(tier: string | undefined): {
  panel: string;
  header: string;
  headerText: string;
} {
  if (tier === 'fully_on_ckb_and_btc') {
    return {
      panel: 'border-[#4a6838]',
      header: 'border-[#4a6838] from-[#4a3a12]/50',
      headerText: 'text-[#a0b880]',
    };
  }
  if (tier === 'fully_on_ckb' || tier === 'fully_onchain') {
    return {
      panel: 'border-[#1a6050]',
      header: 'border-[#1a6050] from-[#0e3830]/50',
      headerText: 'text-[#5abfa0]',
    };
  }
  if (tier === 'centralized_dependent') {
    return {
      panel: 'border-[#5a2020]',
      header: 'border-[#5a2020]',
      headerText: 'text-rouge-dim',
    };
  }
  return {
    panel: '',
    header: '',
    headerText: '',
  };
}

export function StorageTierTooltip({
  tier,
  buttonClassName,
}: {
  tier: string;
  buttonClassName?: string;
}) {
  const text = STORAGE_TIER_DESCRIPTIONS[tier] || STORAGE_TIER_DESCRIPTIONS.unknown;
  return <Tooltip text={text} buttonClassName={buttonClassName} />;
}
