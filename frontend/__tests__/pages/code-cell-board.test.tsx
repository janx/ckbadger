import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import CellDetailPage from '@/app/cell/[outpoint]/client-page';
import { api, type Cell } from '@/lib/api';
import { elfParts } from '@/lib/cell-parts';
import codeCellFixture from '@/__tests__/fixtures/d5/cell-code-board.json';

// The CellCode board's cell: JoyID v5, 0x673e1843…55f9:0 — 100,438 CKB, a 100,312-byte RISC-V
// ELF. Its layout is the chain's (program headers, sections, the 4-byte gap); only the payload
// bytes are zero here, which the layout does not depend on.
function elfBytes(): string {
  const head = '7f454c46020101000000000000000000' + '0200' + 'f300' + '01000000';
  return `0x${head}${'00'.repeat(100312 - head.length / 2)}`;
}
const cell = { ...codeCellFixture, data: elfBytes() } as unknown as Cell;
const MULTISIG = cell.lock!.codeHash;
vi.mock('@/src/navigation', () => ({
  useParams: () => ({ outpoint: `${codeCellFixture.txHash}-0` }),
  usePathname: () => '/cell',
  useRouter: () => ({ push: vi.fn() }),
}));
vi.mock('@/components/layout/header', () => ({ Header: () => <div /> }));
vi.mock('@/components/cell/cell-transactions', () => ({
  CellTransactions: () => <div data-testid="transactions" />,
}));
vi.mock('@/lib/api', () => ({
  api: { getCell: vi.fn(), lookupScripts: vi.fn() },
  isNotFoundApiError: () => false,
}));

function view() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <CellDetailPage />
    </QueryClientProvider>
  );
}
const wait = { timeout: 5000 };

describe('code cell page against the CellCode board', () => {
  beforeEach(() => {
    vi.mocked(api.getCell).mockResolvedValue(cell);
    vi.mocked(api.lookupScripts).mockResolvedValue({ [MULTISIG]: { name: 'Multisig' } } as never);
  });

  it('says whose code it is in the header and names lock and type (SC-K2)', async () => {
    view();
    expect(await screen.findByTestId('code-cell-of', undefined, wait)).toHaveTextContent(
      '· code cell of JoyID, v5'
    );
    const facts = screen.getByRole('region', { name: 'Cell facts' });
    await within(facts).findByRole('link', { name: 'Multisig' }, wait);
    expect(facts).toHaveTextContent('args 0x6598…d696');
    expect(facts).toHaveTextContent('TypeType ID');
    expect(facts).toHaveTextContent('Data100,312 BRISC-V ELF');
    expect(facts.textContent).not.toContain(
      '0x00000000000000000000000000000000000000000000000000545950455f4944'
    );
  });

  it('puts "This cell is code" before the microscope: by type hash, by data hash, built with (SC-K3)', async () => {
    view();
    const code = await screen.findByRole('heading', { name: 'This cell is code' }, wait);
    const microscope = screen.getByRole('heading', { name: 'The cell, byte by byte' });
    expect(
      code.compareDocumentPosition(microscope) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
    const section = code.closest('section')!;
    const byType = within(section).getByTestId('code-by-type-hash');
    expect(byType).toHaveTextContent('by type hash · hash_type type');
    expect(byType).toHaveTextContent(
      '0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323'
    );
    expect(byType).toHaveTextContent(
      '161,049 live JoyID cells name it this way. They follow upgrades'
    );
    const byData = within(section).getByTestId('code-by-data-hash');
    expect(byData).toHaveTextContent('by data hash · hash_type data1 / data2');
    expect(byData).toHaveTextContent(codeCellFixture.codeCell.versionHash);
    expect(byData).toHaveTextContent('0 live cells pin this exact binary');
    const built = within(section).getByTestId('code-built-with');
    expect(built).toHaveTextContent('rustc version 1.76.0-nightly (9fad68599 2023-12-03)');
    expect(built).toHaveTextContent('Linker: LLD 17.0.5');
    expect(built).toHaveTextContent('target rv64i2p1_m2p0_a2p1_c2p0');
    expect(built).toHaveTextContent('entry 0x12de4');
    // the microscope names every part; the ELF tables are not repeated
    expect(screen.queryByRole('table')).toBeNull();
    expect(screen.queryByText('Program headers')).toBeNull();
  });

  it('reads the data as an ELF to scale, 36 parts, with excerpts instead of pages (SC-K4)', async () => {
    view();
    const map = await screen.findByRole('group', { name: 'ELF binary map' }, wait);
    expect(within(map).getByTitle('.text · 91,352 B')).toHaveTextContent('.text 91,352');
    expect(within(map).getByTitle('ELF header · 64 B')).toBeInTheDocument();
    expect(within(map).getByTitle('program headers · 336 B')).toBeInTheDocument();
    const index = screen.getByLabelText('Field index');
    for (const name of [
      'e_machine',
      'e_entry',
      'program header 2 · LOAD',
      'program header 3 · LOAD',
      '.text',
      '.comment',
      'alignment padding',
      'section header table',
    ])
      expect(
        within(index).getByRole('button', {
          name: new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')} \\[`),
        })
      ).toBeInTheDocument();
    expect(index).toHaveTextContent(
      '+ 19 more ELF header fields and program headers: hover their bytes'
    );
    const data = screen.getByLabelText('CellData byte map');
    expect(data).toHaveTextContent('⋯ 98,904 bytes [408..99,312)');
    expect(data).toHaveTextContent('⋯ 760 bytes [99,552..100,312): section header table');
    expect(screen.queryByRole('button', { name: 'Next bytes' })).toBeNull();
    fireEvent.mouseOver(data.querySelector('[data-byte="data:18"]')!);
    const detail = screen.getByRole('complementary', { name: 'Field detail' });
    expect(detail).toHaveTextContent('e_machine');
    expect(detail).toHaveTextContent('valueRISC-V');
    expect(detail).toHaveTextContent('typeELF header');
  });

  it('keeps small map parts visible and writes counts with separators (SC-K6)', async () => {
    view();
    const map = await screen.findByRole('group', { name: 'Capacity map' }, wait);
    for (const part of ['capacity', 'lock', 'type'])
      expect(
        (within(map).getByTitle(new RegExp(`^${part} · `)) as HTMLElement).style.minWidth
      ).toBe('28px');
    expect(screen.getByText(/observed block #11,132,140/)).toBeInTheDocument();
  });
});

describe('elfParts (JoyID v5)', () => {
  it('covers all 100,312 bytes with 36 parts', () => {
    const parts = elfParts(cell);
    expect(parts).toHaveLength(36);
    expect(parts.filter((p) => p.type === 'ELF header')).toHaveLength(19);
    expect(parts.filter((p) => p.type === 'Elf64_Phdr')).toHaveLength(6);
    expect(parts.filter((p) => p.type === 'section')).toHaveLength(9);
    expect(parts.at(-1)).toMatchObject({ label: 'section header table', end: 100312 });
    expect(parts.find((p) => p.label === 'e_type')!.value).toBe('2 = EXEC');
  });
});
