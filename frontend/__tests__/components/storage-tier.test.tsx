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
