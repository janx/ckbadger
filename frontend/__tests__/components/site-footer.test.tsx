import { render, screen } from '../utils/test-utils';
import { describe, expect, it } from 'vitest';
import { SiteFooter } from '@/components/layout/site-footer';

describe('SiteFooter', () => {
  it('shows keyboard shortcut hints', () => {
    render(<SiteFooter />);

    expect(screen.getByText('Shortcuts')).toBeInTheDocument();
    expect(screen.getByText('/ Search')).toBeInTheDocument();
    expect(screen.getByText('Ctrl/Cmd+K Commands')).toBeInTheDocument();
    expect(screen.getByText('? Help')).toBeInTheDocument();
    expect(screen.getByText('g b Blocks')).toBeInTheDocument();
    expect(screen.getByText('g t Transactions')).toBeInTheDocument();
    expect(screen.getByText('g d DAO')).toBeInTheDocument();
    expect(screen.getByText('g a Assets')).toBeInTheDocument();
  });
});
