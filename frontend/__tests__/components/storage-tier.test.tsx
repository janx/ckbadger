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
      ['btc_ckb', '#a0b880', '#c0d8a0', '#4a6838', '#6a8850'],
      ['pure_ckb', '#5abfa0', '#40e8b0', '#1a6050', '#2a8068'],
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
      panelStyle: { borderColor: '#4a6838' },
      header: 'from-[color:var(--tier-wash)]',
      headerStyle: { borderColor: '#4a6838', '--tier-wash': 'rgba(74, 58, 18, 0.5)' },
      headerText: '',
      headerTextStyle: { color: '#a0b880' },
    });
    expect(previewPanelStyle('fully_onchain')).toEqual(previewPanelStyle('pure_ckb'));
    expect(previewPanelStyle('pure_ckb').headerStyle).toEqual({
      borderColor: '#1a6050',
      '--tier-wash': 'rgba(14, 56, 48, 0.5)',
    });
    expect(previewPanelStyle('centralized_mixture')).toEqual({
      panel: '',
      panelStyle: { borderColor: '#5a2020' },
      header: '',
      headerStyle: { borderColor: '#5a2020' },
      headerText: 'text-rouge-dim',
    });
  });
});
