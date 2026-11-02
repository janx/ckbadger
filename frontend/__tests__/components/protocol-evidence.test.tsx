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
  it('draws the bytes to scale, one part per field and one per run no field describes (C5)', () => {
    const { container } = render(
      <ProtocolEvidence title="DID cell data" dataHex="0x01aabbcc" fields={fields} />
    );
    const map = screen.getByRole('group', { name: 'DID cell data byte map' });
    expect(
      [...map.querySelectorAll('[data-segment]')].map((s) => s.getAttribute('data-target'))
    ).toEqual(['field-0', 'gap:1-2', 'field-1']);
    expect(container.querySelectorAll('[data-byte-offset]')).toHaveLength(0);
    expect(
      screen.getByText('4 bytes · 2 described fields · 1 byte no field describes')
    ).toBeInTheDocument();
    expect(screen.queryByText(/unexplained/)).not.toBeInTheDocument();
  });

  it('explains the hovered or tapped field in one detail box (C5)', () => {
    render(<ProtocolEvidence title="DID cell data" dataHex="0x01aabbcc" fields={fields} />);
    const detail = screen.getByRole('complementary', { name: 'DID cell data field detail' });
    // nothing hovered: the first field
    expect(within(detail).getByText('Version')).toBeInTheDocument();
    expect(within(detail).getByText('[0..1) · 1 B')).toBeInTheDocument();
    const document = screen.getByRole('button', { name: /^Document · \[2\.\.4\)/ });
    fireEvent.mouseEnter(document);
    expect(detail).toHaveAttribute('data-detail', 'field-1');
    expect(within(detail).getByText('Document')).toBeInTheDocument();
    expect(within(detail).getByText('[2..4) · 2 B')).toBeInTheDocument();
    expect(within(detail).getByText('bbcc')).toBeInTheDocument();
    fireEvent.mouseLeave(screen.getByRole('group', { name: 'DID cell data byte map' }));
    expect(detail).toHaveAttribute('data-detail', 'field-0');
    // a tap pins it
    fireEvent.click(document);
    expect(document).toHaveAttribute('aria-pressed', 'true');
    fireEvent.mouseLeave(screen.getByRole('group', { name: 'DID cell data byte map' }));
    expect(detail).toHaveAttribute('data-detail', 'field-1');
    // the run no field describes has its own reading
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^\[1\.\.2\) · 1 B · no field/ }));
    expect(within(detail).getByText('1 byte no field describes')).toBeInTheDocument();
    expect(within(detail).getByText('aa')).toBeInTheDocument();
  });

  it('starts the detail over when the data changes (a version switch keeps no stale pin)', () => {
    const { rerender } = render(
      <ProtocolEvidence title="DID cell data" dataHex="0x01aabbcc" fields={fields} />
    );
    fireEvent.click(screen.getByRole('button', { name: /^\[1\.\.2\) · 1 B · no field/ }));
    expect(
      screen.getByRole('complementary', { name: 'DID cell data field detail' })
    ).toHaveAttribute('data-detail', 'gap:1-2');
    // the next version has no undescribed run at [1..2)
    rerender(
      <ProtocolEvidence
        title="DID cell data"
        dataHex="0x01bbcc"
        fields={[
          { name: 'Version', start: 0, end: 1, valueHex: '0x01' },
          { name: 'Document', start: 1, end: 3, valueHex: '0xbbcc' },
        ]}
      />
    );
    expect(
      screen.getByRole('complementary', { name: 'DID cell data field detail' })
    ).toHaveAttribute('data-detail', 'field-0');
  });

  it('gives a field its meaning and the shape its bytes show', () => {
    const name = '0b0000002e63656c6c206465656473';
    render(
      <ProtocolEvidence
        title="Cluster"
        dataHex={`0x${name}`}
        fields={[
          {
            name: 'name',
            start: 0,
            end: 15,
            valueHex: `0x${name}`,
            meaning: 'The cluster’s name.',
          },
        ]}
      />
    );
    const detail = screen.getByRole('complementary', { name: 'Cluster field detail' });
    expect(
      within(detail).getByText(
        'The cluster’s name. A 4-byte length (11), then 11 bytes of UTF-8 text.'
      )
    ).toBeInTheDocument();
    // 4-byte words, the length word dim, printable ASCII beside them
    expect(within(detail).getByText('0b000000')).toHaveAttribute('data-dim', 'true');
    expect(within(detail).getByText('2e63656c')).toHaveAttribute('data-dim', 'false');
    expect(within(detail).getByText('.....cell deeds')).toBeInTheDocument();
  });

  it('names a molecule table header only when the bytes prove it', () => {
    // total 24 · header 12 · fields at 12 and 20
    const dataHex = '0x180000000c00000014000000' + '04000000616263640000000' + '0';
    render(
      <ProtocolEvidence
        title="Table"
        dataHex={dataHex}
        fields={[
          { name: 'a', start: 12, end: 20, valueHex: '0x0400000061626364' },
          { name: 'b', start: 20, end: 24, valueHex: '0x00000000' },
        ]}
      />
    );
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^\[0\.\.12\)/ }));
    const detail = screen.getByRole('complementary', { name: 'Table field detail' });
    expect(within(detail).getByText('molecule table header')).toBeInTheDocument();
    expect(
      within(detail).getByText(
        'No field describes these bytes: a molecule table header, the size of all 24 bytes, then where its 2 fields start: 12, 20.'
      )
    ).toBeInTheDocument();
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
      screen.getByText('3 bytes · 2 described fields · 0 bytes no field describes')
    ).toBeInTheDocument();
    expect(screen.getByTitle('Full, Tail · [1..3) · 2 B')).toBeInTheDocument();
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
    const detail = screen.getByRole('complementary', { name: 'Empty data field detail' });
    expect(within(detail).getByText('Optional local ID')).toBeInTheDocument();
    expect(within(detail).getByText('[0..0) · 0 B')).toBeInTheDocument();
    expect(within(detail).getByText('Empty: no bytes.')).toBeInTheDocument();
    expect(container.querySelectorAll('[data-segment]')).toHaveLength(0);
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
    expect(screen.getByTitle('Bytes · [0..2) · 2 B')).toBeInTheDocument();
    expect(
      within(screen.getByRole('complementary', { name: 'Case field detail' })).getByText('aabb')
    ).toBeInTheDocument();
    const raw = screen.getByText('All 2 bytes as hex').closest('details')!;
    expect(raw).not.toHaveAttribute('open');
    expect(within(raw).getByText('0xAABB')).toBeInTheDocument();
  });

  it('draws a large payload whole and folds a field longer than 64 bytes (C5)', () => {
    const dataHex = '0x' + '00'.repeat(255) + 'aabbccdd';
    const { container } = render(
      <ProtocolEvidence
        title="Large data"
        dataHex={dataHex}
        fields={[
          { name: 'Boundary', start: 255, end: 258, valueHex: 'aabbcc' },
          { name: 'Long', start: 0, end: 100, valueHex: '00'.repeat(100) },
        ]}
      />
    );
    expect(screen.queryByRole('navigation')).not.toBeInTheDocument();
    expect(screen.queryByText(/Bytes 0–255/)).not.toBeInTheDocument();
    expect(
      [...container.querySelectorAll('[data-segment]')].map((s) => s.getAttribute('data-target'))
    ).toEqual(['field-1', 'gap:100-255', 'field-0', 'gap:258-259']);
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^Long · \[0\.\.100\)/ }));
    const detail = screen.getByRole('complementary', { name: 'Large data field detail' });
    // 32 bytes in two rows, then the rest folded with its full hex
    expect(detail.querySelectorAll('[data-hex-row]')).toHaveLength(2);
    expect(within(detail).getByText('… 68 more bytes')).toBeInTheDocument();
    const all = within(detail).getByText('All 100 bytes of Long').closest('details')!;
    expect(all).not.toHaveAttribute('open');
    expect(all.querySelector('pre')).toHaveTextContent('0x' + '00'.repeat(100));
    // a field of 64 bytes or fewer is shown whole
    fireEvent.mouseEnter(screen.getByRole('button', { name: /^Boundary · \[255\.\.258\)/ }));
    expect(detail.querySelectorAll('[data-hex-row]')).toHaveLength(1);
    expect(within(detail).queryByText(/more bytes/)).not.toBeInTheDocument();
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
      within(screen.getByText('All 2 bytes as hex').closest('details')!).getByText('0x0102')
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

// F2 identities (round 2): ID-C12 / TX-16 — a tier-2 heading is 13 px in the quiet text colour.
describe('protocol byte evidence in tier 2', () => {
  it('takes the quiet tier-2 heading inside DetailTier2 and keeps the tier-1 one outside', async () => {
    const { DetailTier2 } = await import('@/components/ui/detail-layout');
    const { unmount } = render(
      <DetailTier2>
        <ProtocolEvidence title="Account cell data" dataHex="0x01aabbcc" fields={fields} />
      </DetailTier2>
    );
    expect(screen.getByRole('heading', { name: 'Account cell data' })).toHaveClass(
      'text-body',
      'text-text'
    );
    unmount();
    render(<ProtocolEvidence title="Account cell data" dataHex="0x01aabbcc" fields={fields} />);
    expect(screen.getByRole('heading', { name: 'Account cell data' })).toHaveClass(
      'text-section',
      'text-text-bright'
    );
  });
});
