import { describe, expect, it } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import {
  CompositionTierTooltip,
  compositionTierCardStyle,
  previewPanelStyle,
} from '@/components/object/storage-tier';

describe('pending storage composition', () => {
  it('keeps pending cards and previews neutral until DOB execution supplies evidence', () => {
    const style = compositionTierCardStyle('decode_pending');
    expect(style.text).toBe('text-text-dim');
    expect(style.card).not.toMatch(/storage-card-ckb|storage-card-both/);
    expect(previewPanelStyle('decode_pending')).toEqual({ panel: '', header: '', headerText: '' });
  });
  it('explains pending DOB execution instead of reporting unknown composition', () => {
    render(<CompositionTierTooltip tier="decode_pending" />);
    fireEvent.focus(screen.getByRole('button', { name: 'More info' }));
    expect(
      screen.getByText('Awaiting DOB execution. Storage composition has not been classified yet.')
    ).toBeInTheDocument();
    expect(screen.queryByText(/Composition could not be determined/)).not.toBeInTheDocument();
  });
});

// Visual finish V11: a tier's own inks are named (STORAGE_TIER_COLORS), never bracket hex; the
// theme's values are theme classes; the tooltip button's hover states read CSS variables.
describe('storage tier inks (V11)', () => {
  it('draws the on-chain tiers from named inks, the frame from theme classes', () => {
    for (const [tier, ink, inkLit, edge, edgeLit] of [
      [
        'btc_ckb',
        'var(--c-tier-btc-ink)',
        'var(--c-tier-btc-ink-lit)',
        'var(--c-tier-btc-edge)',
        'var(--c-tier-btc-edge-lit)',
      ],
      [
        'pure_ckb',
        'var(--c-tier-pure-ink)',
        'var(--c-tier-pure-ink-lit)',
        'var(--c-tier-pure-edge)',
        'var(--c-tier-pure-edge-lit)',
      ],
    ] as const) {
      const style = compositionTierCardStyle(tier);
      expect(style.card).toContain('border-base-border bg-base-surface');
      expect(style.labelStyle).toEqual({ color: ink });
      expect(style.tooltipButton).toContain(
        'text-[color:var(--tier-ink)] border-[color:var(--tier-edge)] hover:text-[color:var(--tier-ink-lit)] hover:border-[color:var(--tier-edge-lit)]'
      );
      expect(style.tooltipButtonStyle).toEqual({
        '--tier-ink': ink,
        '--tier-edge': edge,
        '--tier-ink-lit': inkLit,
        '--tier-edge-lit': edgeLit,
      });
      expect(JSON.stringify(style)).not.toMatch(/-\[#/);
    }
  });

  it('frames the preview in the tier edge and washes its header from the tier colour at 50 %', () => {
    expect(previewPanelStyle('btc_ckb')).toEqual({
      panel: '',
      panelStyle: { borderColor: 'var(--c-tier-btc-edge)' },
      header: 'from-[color:var(--tier-wash)]',
      headerStyle: {
        borderColor: 'var(--c-tier-btc-edge)',
        '--tier-wash': 'rgb(var(--rgb-tier-btc-wash) / 0.5)',
      },
      headerText: '',
      headerTextStyle: { color: 'var(--c-tier-btc-ink)' },
    });
    expect(previewPanelStyle('fully_onchain')).toEqual(previewPanelStyle('pure_ckb'));
    expect(previewPanelStyle('pure_ckb').headerStyle).toEqual({
      borderColor: 'var(--c-tier-pure-edge)',
      '--tier-wash': 'rgb(var(--rgb-tier-pure-wash) / 0.5)',
    });
    expect(previewPanelStyle('centralized_mixture')).toEqual({
      panel: '',
      panelStyle: { borderColor: 'var(--c-tier-centralized-edge)' },
      header: '',
      headerStyle: { borderColor: 'var(--c-tier-centralized-edge)' },
      headerText: 'text-rouge-dim',
    });
  });
});
