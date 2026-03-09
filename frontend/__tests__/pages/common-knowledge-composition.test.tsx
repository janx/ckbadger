import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import CommonKnowledgeCompositionPage from '@/app/charts/common-knowledge-composition/page';
import { api } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  api: {
    getCommonKnowledgeCompositionChart: vi.fn(),
  },
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: () => <div data-testid="stacked-area-chart" />,
}));

describe('CommonKnowledgeCompositionPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getCommonKnowledgeCompositionChart).mockResolvedValue({
      title: 'Common Knowledge Bytes Composition',
      data: [
        {
          date: '2026-02-23',
          values: {
            occupied: '100',
            unoccupied: '50',
          },
        },
      ],
      series: [
        { key: 'occupied', label: 'Occupied', color: '#00c389' },
        { key: 'unoccupied', label: 'Unoccupied', color: '#64748b' },
      ],
    });
  });

  it('renders page title and stacked chart', async () => {
    render(<CommonKnowledgeCompositionPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Common Knowledge Bytes Composition')).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId('stacked-area-chart')).toBeInTheDocument();
      expect(screen.getByText('Occupied')).toBeInTheDocument();
      expect(screen.getByText('Unoccupied')).toBeInTheDocument();
      expect(screen.getByText(/Drag to select range/i)).toHaveClass('text-text-muted');
    });
  });
});
