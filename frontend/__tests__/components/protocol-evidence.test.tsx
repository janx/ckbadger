import { afterEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, waitFor, within } from '@/__tests__/utils/test-utils';
import { ProtocolEvidence } from '@/components/asset/protocol-evidence';
import { HexMap } from '@/components/ui/hex-map';

const fields = [
  { name: 'Version', start: 0, end: 1, valueHex: '0x01' },
  { name: 'Document', start: 2, end: 4, valueHex: '0xbbcc' },
];

const clipboardDescriptor = Object.getOwnPropertyDescriptor(navigator, 'clipboard');
afterEach(() => {
  vi.restoreAllMocks();
  if (clipboardDescriptor) Object.defineProperty(navigator, 'clipboard', clipboardDescriptor);
  else Reflect.deleteProperty(navigator, 'clipboard');
});

describe('protocol byte evidence', () => {
  it('keeps described and unexplained bytes, with field and byte focus association', () => {
    const { container } = render(
      <ProtocolEvidence title="DID cell data" dataHex="0x01aabbcc" fields={fields} />
    );
    expect(container.querySelectorAll('[data-byte-offset]')).toHaveLength(4);
    expect(
      screen.getByText('4 bytes · 2 described fields · 1 unexplained bytes')
    ).toBeInTheDocument();
    fireEvent.focus(screen.getByRole('button', { name: 'Document bytes 2–3' }));
    expect(container.querySelector('[data-byte-offset="2"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-byte-offset="0"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
    expect(container.querySelector('[data-field-index="1"]')).toHaveAttribute(
      'data-active',
      'true'
    );
    fireEvent.blur(screen.getByRole('button', { name: 'Document bytes 2–3' }));
    fireEvent.mouseEnter(screen.getByTitle('Byte 0: 0x01 · Version'));
    expect(container.querySelector('[data-field-index="0"]')).toHaveAttribute(
      'data-active',
      'true'
    );
    fireEvent.mouseLeave(screen.getByTitle('Byte 0: 0x01 · Version'));
    fireEvent.focus(screen.getByTitle('Byte 1: 0xaa'));
    expect(container.querySelectorAll('[data-field-index][data-active="true"]')).toHaveLength(0);
    expect(container.querySelectorAll('[data-byte-offset][data-highlighted="true"]')).toHaveLength(
      0
    );
  });

  it('accepts overlapping evidence without double-counting explained bytes', () => {
    render(
      <ProtocolEvidence
        title="Overlapping data"
        dataHex="0x010203"
        fields={[
          { name: 'Full', start: 0, end: 3, valueHex: '010203' },
          { name: 'Tail', start: 1, end: 3, valueHex: '0x0203' },
        ]}
      />
    );
    expect(
      screen.getByText('3 bytes · 2 described fields · 0 unexplained bytes')
    ).toBeInTheDocument();
    expect(screen.getByTitle('Byte 1: 0x02 · Full, Tail')).toBeInTheDocument();
  });

  it('preserves empty payloads and zero-width optional fields explicitly', () => {
    const { container } = render(
      <ProtocolEvidence
        title="Empty data"
        dataHex="0x"
        fields={[{ name: 'Optional local ID', start: 0, end: 0, valueHex: '0x' }]}
      />
    );
    expect(screen.getByText('Empty cell data (0 bytes).')).toBeInTheDocument();
    expect(screen.getByText('Optional local ID · [0, 0) · empty field')).toBeInTheDocument();
    expect(container.querySelectorAll('[data-byte-offset]')).toHaveLength(0);
  });

  it.each([
    { name: 'negative', start: -1, end: 1, valueHex: '01' },
    { name: 'reverse', start: 2, end: 1, valueHex: '' },
    { name: 'outside', start: 0, end: 5, valueHex: '0102030405' },
    { name: 'fraction', start: 0.5, end: 1, valueHex: '01' },
    {
      name: 'unsafe',
      start: Number.MAX_SAFE_INTEGER + 1,
      end: Number.MAX_SAFE_INTEGER + 1,
      valueHex: '',
    },
    { name: '', start: 0, end: 1, valueHex: '01' },
  ])('rejects invalid $name range with item context', (field) => {
    expect(() =>
      render(<ProtocolEvidence title="Cell evidence 0xabc:2" dataHex="01020304" fields={[field]} />)
    ).toThrow('Cell evidence 0xabc:2: invalid field');
  });

  it('rejects mismatched field values, malformed field hex and malformed cell bytes', () => {
    expect(() =>
      render(
        <ProtocolEvidence
          title="Cell data"
          dataHex="0102"
          fields={[{ name: 'Wrong', start: 0, end: 1, valueHex: '02' }]}
        />
      )
    ).toThrow('field Wrong value does not match bytes [0, 1)');
    expect(() =>
      render(
        <ProtocolEvidence
          title="Cell data"
          dataHex="0102"
          fields={[{ name: 'Bad hex', start: 0, end: 1, valueHex: '0x1' }]}
        />
      )
    ).toThrow('field Bad hex: expected byte-aligned');
    expect(() =>
      render(<ProtocolEvidence title="Cell data" dataHex="not-hex" fields={[]} />)
    ).toThrow('Cell data: expected byte-aligned');
  });

  it('normalizes only hex case and prefix while preserving the original raw evidence', () => {
    render(
      <ProtocolEvidence
        title="Case"
        dataHex="0xAABB"
        fields={[{ name: 'Bytes', start: 0, end: 2, valueHex: 'aabb' }]}
      />
    );
    expect(screen.getByTitle('Byte 0: 0xaa · Bytes')).toBeInTheDocument();
    const raw = screen.getByText('Complete raw hex').closest('details')!;
    expect(raw).not.toHaveAttribute('open');
    expect(within(raw).getByText('0xAABB')).toBeInTheDocument();
  });

  it('pages large payloads with absolute offsets and reaches every byte', () => {
    const dataHex = '0x' + '00'.repeat(255) + 'aabbccdd';
    const { container, rerender } = render(
      <ProtocolEvidence
        title="Large data"
        dataHex={dataHex}
        fields={[{ name: 'Boundary', start: 255, end: 258, valueHex: 'aabbcc' }]}
      />
    );
    expect(container.querySelectorAll('[data-byte-offset]')).toHaveLength(256);
    expect(screen.getByText('Bytes 0–255 of 259')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Previous bytes' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Boundary bytes 255–255' })).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Next bytes' }));
    expect(container.querySelectorAll('[data-byte-offset]')).toHaveLength(3);
    expect(screen.getByText('Bytes 256–258 of 259')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Next bytes' })).toBeDisabled();
    fireEvent.focus(screen.getByRole('button', { name: 'Boundary bytes 256–257' }));
    expect(container.querySelector('[data-byte-offset="256"]')).toHaveAttribute(
      'data-highlighted',
      'true'
    );
    expect(container.querySelector('[data-byte-offset="258"]')).toHaveAttribute(
      'data-highlighted',
      'false'
    );
    expect(screen.getByTitle('Byte 258: 0xdd')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Previous bytes' }));
    expect(screen.getByText('Bytes 0–255 of 259')).toBeInTheDocument();
    fireEvent.click(screen.getByRole('button', { name: 'Next bytes' }));
    rerender(<ProtocolEvidence title="Large data" dataHex="0x01" fields={[]} />);
    expect(screen.getByTitle('Byte 0: 0x01')).toBeInTheDocument();
    expect(
      screen.queryByRole('navigation', { name: 'Large data byte pages' })
    ).not.toBeInTheDocument();
  });

  it('preserves structured decoded evidence, lossless CBOR tags and arbitrary text', () => {
    const decoded = {
      keys: [{ $bytes: '0xaabb' }],
      version: { $integer: '18446744073709551615' },
      tag: { $tag: '18446744073709551615', value: null },
      text: '<script>alert(1)</script>',
    };
    render(
      <ProtocolEvidence title="Document" dataHex="0x00" fields={[]} decoded={decoded}>
        <p>CBOR document provenance</p>
      </ProtocolEvidence>
    );
    expect(screen.getByText('CBOR document provenance')).toBeInTheDocument();
    const details = screen.getByText('Decoded fields').closest('details')!;
    expect(details).not.toHaveAttribute('open');
    const pre = details.querySelector('pre')!;
    expect(JSON.parse(pre.textContent!)).toEqual(decoded);
    expect(pre).toHaveClass('whitespace-pre-wrap', 'break-all', 'max-w-full');
    expect(pre.querySelector('script')).toBeNull();
    expect(
      screen.getByText('Protocol interpretation; raw cell bytes remain the evidence.')
    ).toBeInTheDocument();
  });

  it('keeps decoded null distinct from missing data and rejects non-finite numbers', () => {
    const { rerender } = render(
      <ProtocolEvidence title="Null" dataHex="0x00" fields={[]} decoded={null} />
    );
    expect(
      screen.getByText('Decoded fields').closest('details')!.querySelector('pre')
    ).toHaveTextContent('null');
    rerender(<ProtocolEvidence title="Null" dataHex="0x00" fields={[]} />);
    expect(screen.queryByText('Decoded fields')).not.toBeInTheDocument();
    expect(() =>
      render(
        <ProtocolEvidence
          title="Unsafe CBOR"
          dataHex="0x00"
          fields={[]}
          decoded={{ n: Number.NaN }}
        />
      )
    ).toThrow('decoded numeric value is not finite');
  });

  it('does not reinterpret original finite floating values as tagged CBOR integers', () => {
    render(
      <ProtocolEvidence
        title="Float document"
        dataHex="0x00"
        fields={[]}
        decoded={{ originalFloat: 1e20, originalInteger: { $integer: '100000000000000000001' } }}
      />
    );
    const value = JSON.parse(
      screen.getByText('Decoded fields').closest('details')!.querySelector('pre')!.textContent!
    );
    expect(value.originalFloat).toBe(1e20);
    expect(value.originalInteger.$integer).toBe('100000000000000000001');
  });

  it('copies the complete original bytes, including bytes outside the visible page', async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(navigator, 'clipboard', { configurable: true, value: { writeText } });
    const value = '0x' + 'AB'.repeat(300);
    const { rerender } = render(<ProtocolEvidence title="Copy" dataHex={value} fields={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy raw bytes' }));
    await waitFor(() => expect(writeText).toHaveBeenCalledWith(value));
    expect(screen.getByRole('status')).toHaveTextContent('Raw bytes copied.');
    rerender(<ProtocolEvidence title="Copy" dataHex="0xcd" fields={[]} />);
    expect(screen.queryByText('Raw bytes copied.')).not.toBeInTheDocument();
  });

  it('exposes a clipboard failure and retains raw hex for inspection', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn().mockRejectedValue(new Error('clipboard denied')) },
    });
    render(<ProtocolEvidence title="Copy failure" dataHex="0x0102" fields={[]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Copy raw bytes' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'Could not copy raw bytes: clipboard denied'
    );
    expect(
      within(screen.getByText('Complete raw hex').closest('details')!).getByText('0x0102')
    ).toBeInTheDocument();
  });

  it('rejects unsafe byte-map base offsets before displaying rounded positions', () => {
    expect(() =>
      render(
        <HexMap hex="01" fields={[]} subject="Paged cell" offsetBase={Number.MAX_SAFE_INTEGER} />
      )
    ).toThrow('invalid byte-map base offset');
    expect(() =>
      render(<HexMap hex="01" fields={[]} subject="Paged cell" offsetBase={-1} />)
    ).toThrow('invalid byte-map base offset');
  });
});
