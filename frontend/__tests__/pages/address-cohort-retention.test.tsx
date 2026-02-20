import { describe, expect, it, vi } from 'vitest';
import { render, screen } from '../utils/test-utils';
import AddressCohortRetentionPage from '@/app/charts/address-cohort-retention/page';

vi.mock('@/lib/api', () => ({
  api: {
    getAddressCohortRetentionChart: vi.fn(),
  },
}));

vi.mock('@/components/charts/chart-page', () => ({
  ChartPage: ({ title, queryKey }: { title: string; queryKey: string }) => (
    <div>
      <span>{title}</span>
      <span>{queryKey}</span>
    </div>
  ),
}));

describe('AddressCohortRetentionPage', () => {
  it('passes chart title and query key to ChartPage', () => {
    render(<AddressCohortRetentionPage />);
    expect(screen.getByText('Address Cohort Retention')).toBeInTheDocument();
    expect(screen.getByText('chart-address-cohort-retention')).toBeInTheDocument();
  });
});
