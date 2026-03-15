import { beforeEach, describe, expect, it, vi } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { render } from '../utils/test-utils';
import CommonKnowledgeCompositionPage from '@/app/charts/common-knowledge-composition/page';
import { api } from '@/lib/api';

const stackedAreaChartMock = vi.fn((_: unknown) => <div data-testid="stacked-area-chart" />);

vi.mock('@/lib/api', () => ({
  api: {
    getCommonKnowledgeCompositionChart: vi.fn(),
  },
  isWarmupPendingError: vi.fn(() => false),
}));

vi.mock('@/components/layout/header', () => ({
  Header: () => <div data-testid="header">Header</div>,
}));

vi.mock('@/components/ui/stacked-area-chart', () => ({
  StackedAreaChart: (props: unknown) => stackedAreaChartMock(props),
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
            used: '100',
            unused: '50',
          },
        },
      ],
      series: [
        { key: 'used', label: 'Used', color: '#00c389' },
        { key: 'unused', label: 'Unused', color: '#64748b' },
      ],
    });
  });

  it('renders page title and stacked chart', async () => {
    render(<CommonKnowledgeCompositionPage />);

    expect(screen.getByTestId('header')).toBeInTheDocument();
    expect(screen.getByText('Common Knowledge Bytes Composition')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: /Back to Charts/i })).toHaveAttribute(
      'href',
      '/charts'
    );

    await waitFor(() => {
      expect(api.getCommonKnowledgeCompositionChart).toHaveBeenCalledTimes(1);
      expect(screen.getByTestId('stacked-area-chart')).toBeInTheDocument();
      expect(screen.getByText('Used')).toBeInTheDocument();
      expect(screen.getByText('Unused')).toBeInTheDocument();
      expect(screen.getByText(/Drag to select range/i)).toBeInTheDocument();
    });

    expect(stackedAreaChartMock).toHaveBeenCalledWith(
      expect.objectContaining({
        height: 400,
        data: [
          {
            date: '2026-02-23',
            values: {
              used: '100',
              unused: '50',
            },
          },
        ],
        series: [
          { key: 'used', label: 'Used', color: '#00c389' },
          { key: 'unused', label: 'Unused', color: '#64748b' },
        ],
      })
    );
  });
});
