import { render, screen } from '../utils/test-utils';
import { describe, expect, it } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';

describe('SiteFooter', () => {
  it('shows shortcut help hint and attribution', () => {
    render(<SiteFooter />);

    expect(screen.getByRole('link', { name: 'Github' })).toHaveAttribute(
      'href',
      'https://github.com/janx/ckbadger'
    );
    expect(screen.getByText('Press ? for shortcuts')).toBeInTheDocument();
    const profileLink = screen.getByRole('link', { name: '@busyforking' });
    expect(profileLink).toHaveAttribute('href', 'https://x.com/busyforking');
    expect(profileLink.parentElement).toHaveTextContent(
      'Built by @busyforking with agents Coco and Dede ❤️'
    );
  });
});
