import { afterEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { stubChartWidth } from '@/__tests__/utils/chart-width';
import { ScriptByteMap, ScriptNames } from '@/components/ui/script-byte-map';

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

const DEFAULT_LOCK = {
  codeHash: '0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8',
  hashType: 'type',
  args: '0xf9367f81226a26acf0978d64a154b7723e117846',
};

// Consistency lane E (C6, Address board tier 2): a script as its 53 occupied bytes — code_hash 32,
// hash_type 1, args N — drawn to scale with the labels inside and an offsets ruler, then three rows.
describe('ScriptByteMap', () => {
  it('draws code_hash 32 · hash_type 1 · args N to scale with an offsets ruler', () => {
    stubChartWidth(600);
    const { container } = render(
      <ScriptByteMap
        kind="lock"
        script={DEFAULT_LOCK}
        name="Default Lock"
        href="/scripts/Default%20Lock"
      />
    );
    expect(
      screen.getByText('Script · 53 occupied bytes in every cell it locks')
    ).toBeInTheDocument();
    const parts = [...container.querySelectorAll<HTMLElement>('[data-segment]')];
    expect(parts.map((p) => [p.dataset.segment, p.textContent])).toEqual([
      ['code_hash', 'code_hash · 32 · Default Lock'],
      ['hash_type', '1'],
      ['args', 'args · 20'],
    ]);
    expect(
      screen.getByRole('img', { name: 'lock script offsets: 0, 32, 33, 53' })
    ).toBeInTheDocument();
  });

  it('gives the three parts one row each: the code linked, the hash type in a sentence, the args', () => {
    render(
      <ScriptByteMap
        kind="lock"
        script={DEFAULT_LOCK}
        name="Default Lock"
        href="/scripts/Default%20Lock"
        argsNote="blake160 of the public key"
      />
    );
    const rows = screen.getByLabelText('lock script parts');
    const code = within(rows).getByText('code_hash').nextElementSibling!;
    expect(within(code as HTMLElement).getByRole('link')).toHaveAttribute(
      'href',
      '/mainnet/scripts/Default%20Lock'
    );
    expect(code).toHaveTextContent(`${DEFAULT_LOCK.codeHash} · Default Lock`);
    expect(within(rows).getByText('hash_type').nextElementSibling).toHaveTextContent(
      'type (0x01) · the code is found by its type id, so it can be upgraded'
    );
    expect(within(rows).getByText('args').nextElementSibling).toHaveTextContent(
      `${DEFAULT_LOCK.args} · blake160 of the public key`
    );
  });

  it('words every hash type and fails fast on one it does not know', () => {
    render(
      <ScriptByteMap
        kind="type"
        script={{ ...DEFAULT_LOCK, hashType: 'data1', args: '0x' }}
        href="/x"
      />
    );
    expect(
      screen.getByText('Script · 33 occupied bytes in every cell it types')
    ).toBeInTheDocument();
    expect(screen.getByText('hash_type').nextElementSibling).toHaveTextContent(
      'data1 (0x02) · the code is found by its data hash and runs on CKB-VM version 1; it cannot change'
    );
    expect(screen.getByText('args').nextElementSibling).toHaveTextContent('empty: no args');
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(
        <ScriptByteMap kind="lock" script={{ ...DEFAULT_LOCK, hashType: 'data9' }} href="/x" />
      )
    ).toThrow('lock script: unknown hash_type data9');
    log.mockRestore();
  });

  it('draws a script family as its shape: the args each cell sets are named, not drawn', () => {
    stubChartWidth(600);
    const { container } = render(<ScriptByteMap kind="lock" script={null} />);
    expect(
      [...container.querySelectorAll<HTMLElement>('[data-segment]')].map((p) => p.dataset.segment)
    ).toEqual(['code_hash', 'hash_type']);
    expect(screen.getByText('Script · 33 bytes, then the args each cell sets')).toBeInTheDocument();
    expect(screen.queryByLabelText('lock script parts')).toBeNull();
  });
});

describe('ScriptNames (Address board: two names for the same script)', () => {
  it('chains the script to its lock hash and to its address, both derived', () => {
    render(
      <ScriptNames
        lockHash="0xe51d098d9577d8ad5cf476059f9d97ca6c497da0c3aa2c389a86595b0dd080b9"
        address="ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsq0exelczgn2y6k0p9udvjs4fdmj8cghs3shprdnn"
      />
    );
    const section = screen.getByRole('region', { name: 'Derived names' });
    expect(
      within(section).getByRole('heading', { name: 'Two names for the same script' })
    ).toBeInTheDocument();
    const chains = within(section).getAllByRole('list');
    expect(chains).toHaveLength(2);
    expect(chains[0]).toHaveTextContent(
      'lock hashblake2b-256 of it; what the indexes are keyed by'
    );
    expect(chains[1]).toHaveTextContent(
      'scriptthe same three parts↓→addressbech32m of them, full format; the text at the top of this page'
    );
    expect(section).toHaveTextContent(
      '0xe51d098d9577d8ad5cf476059f9d97ca6c497da0c3aa2c389a86595b0dd080b9'
    );
    expect(section).toHaveTextContent(
      'Change one byte of args and both names change; the address is not an account, it is this script spelled out.'
    );
  });

  it('keeps the lock hash alone when the lock has no address', () => {
    render(<ScriptNames lockHash={'0x' + 'ab'.repeat(32)} address={null} />);
    expect(screen.getAllByRole('list')).toHaveLength(1);
    expect(
      screen.getByText('Change one byte of args and the lock hash changes.')
    ).toBeInTheDocument();
  });
});
