import type { CSSProperties } from 'react';
import { Tooltip } from '@/components/spore/cluster-description';
import { cssVars, STORAGE_TIER_COLORS } from '@/lib/design-tokens';

const COMPOSITION_TIER_DESCRIPTIONS: Record<string, string> = {
  decode_pending: 'Awaiting DOB execution. Storage composition has not been classified yet.',
  pure_ckb:
    'All content is stored directly on the CKB blockchain (on-chain data or ckbfs://). Fully verifiable and permanent.',
  btc_ckb:
    'Content is stored across both CKB (on-chain data or ckbfs://) and Bitcoin (btcfs://). Fully verifiable and permanent.',
  decentralized_mixture:
    'Some content references external decentralized storage (e.g. IPFS, Arweave). Data persists as long as the external network hosts it.',
  centralized_mixture:
    'Some content depends on centralized servers (http/https). Data availability relies on the server operator.',
  unknown: 'Composition could not be determined. The content storage method is unverified.',
};

const TOOLTIP_BTN_BASE =
  'ml-1 inline-flex h-3.5 w-3.5 items-center justify-center rounded-full border font-mono text-meta leading-none transition-colors';
/** A tiered tooltip button's inks: CSS variables its classes read, so the hover states still apply. */
const TOOLTIP_BTN_TIER =
  'text-[color:var(--tier-ink)] border-[color:var(--tier-edge)] hover:text-[color:var(--tier-ink-lit)] hover:border-[color:var(--tier-edge-lit)]';

function tooltipButtonInks(tier: 'btc_ckb' | 'pure_ckb'): CSSProperties {
  const c = STORAGE_TIER_COLORS[tier];
  return cssVars({
    '--tier-ink': c.ink,
    '--tier-edge': c.edge,
    '--tier-ink-lit': c.inkLit,
    '--tier-edge-lit': c.edgeLit,
  });
}

export function compositionTierCardStyle(tier: string): {
  card: string;
  label: string;
  /** The label's tier ink, where the tier has its own (a named ink, not a class). */
  labelStyle?: CSSProperties;
  text: string;
  tooltipButton?: string;
  tooltipButtonStyle?: CSSProperties;
} {
  if (tier === 'decode_pending') {
    return {
      card: 'border-base-border rounded border p-3',
      label: 'text-text-dim',
      text: 'text-text-dim',
    };
  }
  if (tier === 'btc_ckb') {
    return {
      card: 'storage-card-no-crt storage-card-both rounded border border-base-border bg-base-surface p-3',
      label: '',
      labelStyle: { color: STORAGE_TIER_COLORS.btc_ckb.ink },
      text: 'storage-text-split',
      tooltipButton: `${TOOLTIP_BTN_BASE} ${TOOLTIP_BTN_TIER}`,
      tooltipButtonStyle: tooltipButtonInks('btc_ckb'),
    };
  }
  if (tier === 'pure_ckb' || tier === 'fully_onchain') {
    return {
      card: 'storage-card-no-crt storage-card-ckb rounded border border-base-border bg-base-surface p-3',
      label: '',
      labelStyle: { color: STORAGE_TIER_COLORS.pure_ckb.ink },
      text: 'storage-text-gem',
      tooltipButton: `${TOOLTIP_BTN_BASE} ${TOOLTIP_BTN_TIER}`,
      tooltipButtonStyle: tooltipButtonInks('pure_ckb'),
    };
  }
  if (tier === 'centralized_mixture') {
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
 * applying the storage tier visual language to the panel border and header. A tier's own inks
 * are named (STORAGE_TIER_COLORS) and come as styles beside the classes; the header's gradient
 * start is the `--tier-wash` variable its `from-` class reads.
 */
export function previewPanelStyle(tier: string | undefined): {
  panel: string;
  panelStyle?: CSSProperties;
  header: string;
  headerStyle?: CSSProperties;
  headerText: string;
  headerTextStyle?: CSSProperties;
} {
  const tiered = (c: (typeof STORAGE_TIER_COLORS)['btc_ckb' | 'pure_ckb']) => ({
    panel: '',
    panelStyle: { borderColor: c.edge },
    header: 'from-[color:var(--tier-wash)]',
    headerStyle: { borderColor: c.edge, ...cssVars({ '--tier-wash': c.wash }) },
    headerText: '',
    headerTextStyle: { color: c.ink },
  });
  if (tier === 'btc_ckb') return tiered(STORAGE_TIER_COLORS.btc_ckb);
  if (tier === 'pure_ckb' || tier === 'fully_onchain') return tiered(STORAGE_TIER_COLORS.pure_ckb);
  if (tier === 'centralized_mixture') {
    const edge = { borderColor: STORAGE_TIER_COLORS.centralized_mixture.edge };
    return {
      panel: '',
      panelStyle: edge,
      header: '',
      headerStyle: edge,
      headerText: 'text-rouge-dim',
    };
  }
  return {
    panel: '',
    header: '',
    headerText: '',
  };
}

export function CompositionTierTooltip({
  tier,
  buttonClassName,
}: {
  tier: string;
  buttonClassName?: string;
}) {
  const text = COMPOSITION_TIER_DESCRIPTIONS[tier] || COMPOSITION_TIER_DESCRIPTIONS.unknown;
  return <Tooltip text={text} buttonClassName={buttonClassName} />;
}
