import { fireEvent, render, screen, within } from '@/__tests__/utils/test-utils';
import { MnftRenderer, MnftTokenIdentity } from '@/components/asset/mnft-detail';

const id = '0x' + 'ab'.repeat(20) + '00000014' + '00000063';

describe('m-NFT identity evidence', () => {
  it('maps issuer, class and token index bytes and names the extension bytes no field describes', () => {
    const { container } = render(<MnftTokenIdentity id={id + 'ff'} />);
    expect(
      screen.getByText('29 bytes · 3 described fields · 1 byte no field describes')
    ).toBeInTheDocument();
    expect(
      [...container.querySelectorAll('[data-segment]')].map((s) => s.getAttribute('data-target'))
    ).toEqual(['field-0', 'field-1', 'field-2', 'gap:28-29']);
    // F2 objects (OB-M11): each index field carries its decoded value.
    fireEvent.focus(
      screen.getByRole('button', { name: /^Token index · big-endian = 99 · \[24\.\.28\)/ })
    );
    const detail = screen.getByRole('complementary', { name: 'm-NFT token ID field detail' });
    expect(detail).toHaveAttribute('data-detail', 'field-2');
    expect(within(detail).getByText('[24..28) · 4 B')).toBeInTheDocument();
    expect(
      within(detail).getByText('Which token of the class, a big-endian number. 4 bytes.')
    ).toBeInTheDocument();
    expect(within(detail).getByText('00000063')).toBeInTheDocument();
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
      // F2 objects (OB-M4): the frame says where the picture lives and links the file.
      expect(screen.getByRole('link', { name: 'Open the file ↗' })).toHaveAttribute(
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
