import { MemoryRouter } from 'react-router-dom';
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { ErrorVessels, VesselsCaption } from '@/components/error-vessels';

describe('error artwork background requests', () => {
  it('reads the producer snapshot while the default test socket remains offline', async () => {
    render(
      <MemoryRouter initialEntries={['/mainnet/missing']}>
        <ErrorVessels />
        <VesselsCaption />
      </MemoryRouter>
    );

    expect(await screen.findByRole('link', { name: 'block #2' })).toHaveAttribute(
      'href',
      '/mainnet/blocks/2'
    );
    expect(screen.getByText(/2 live Common knowledge cells from mainnet/)).toBeInTheDocument();
  });
});
