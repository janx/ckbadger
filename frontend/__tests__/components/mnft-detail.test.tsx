import { fireEvent, render, screen } from '@/__tests__/utils/test-utils';
import { MnftRenderer, MnftTokenIdentity } from '@/components/asset/mnft-detail';

const id = '0x' + 'ab'.repeat(20) + '00000014' + '00000063';

describe('m-NFT identity evidence', () => {
  it('maps issuer, class and token index bytes and leaves extension bytes unexplained', () => {
    const { container } = render(<MnftTokenIdentity id={id + 'ff'} />);
    expect(
      screen.getByText('29 bytes · 3 described fields · 1 unexplained bytes')
    ).toBeInTheDocument();
    fireEvent.focus(screen.getByRole('button', { name: 'Token index · big-endian bytes 24–27' }));
    expect(container.querySelector('[data-byte-offset="27"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-byte-offset="23"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
    expect(container.querySelector('[data-byte-offset="28"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
  });
  it('rejects malformed and undersized identity bytes', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    try {
      expect(() => render(<MnftTokenIdentity id="0x123" />)).toThrow('byte-aligned');
      expect(() => render(<MnftTokenIdentity id="0x1234" />)).toThrow('at least 28 bytes');
    } finally {
      log.mockRestore();
    }
  });
});

describe('m-NFT class renderer', () => {
  it.each(['https://example.com/class.png', 'btcfs://inscription/abc'])(
    'keeps %s as a declared link without embedding remote media',
    (renderer) => {
      const { container } = render(<MnftRenderer renderer={renderer} />);
      expect(screen.getByRole('link', { name: 'Open the renderer ↗' })).toHaveAttribute(
        'href',
        renderer
      );
      expect(screen.getByText(renderer)).toBeInTheDocument();
      expect(container.querySelector('img, iframe, object, embed')).toBeNull();
    }
  );
  it('preserves an unsupported renderer declaration without making an executable link', () => {
    render(<MnftRenderer renderer="javascript:alert(1)" />);
    expect(screen.getByText('javascript:alert(1)')).toBeInTheDocument();
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
  it('states when a class has no renderer', () => {
    render(<MnftRenderer renderer={null} />);
    expect(screen.getByText('No class renderer recorded.')).toBeInTheDocument();
  });
});
