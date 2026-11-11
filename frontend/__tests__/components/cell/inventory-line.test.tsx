import { beforeEach, describe, expect, it, vi } from 'vitest';
import { render, screen, within } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { api, type Cell } from '@/lib/api';
import { CellInventoryLine } from '@/components/cell/inventory-line';
import spore from '@/__tests__/fixtures/cell-inventory/spore.json';
import xudt from '@/__tests__/fixtures/cell-inventory/xudt.json';
import xudtNoAmount from '@/__tests__/fixtures/cell-inventory/xudt-no-amount.json';
import dao from '@/__tests__/fixtures/cell-inventory/dao.json';
import daoWithdraw from '@/__tests__/fixtures/cell-inventory/dao-withdraw.json';
import dotbit from '@/__tests__/fixtures/cell-inventory/dotbit.json';
import did from '@/__tests__/fixtures/cell-inventory/did.json';
import mnft from '@/__tests__/fixtures/cell-inventory/mnft.json';

vi.mock('@/lib/api', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@/lib/api')>()),
  api: {
    getSporeObject: vi.fn(),
    getSporeCluster: vi.fn(),
    getMnftItemDetail: vi.fn(),
    getObjectCollection: vi.fn(),
    getItemProtocol: vi.fn(),
  },
}));

const SPORE_ID = '0x7e3bc1bce9aa1f5d085911dee7673fe54ece2d205f6a796a773d9e6f15216539';
const CLUSTER_ID = '0x95a56c18700cbc3f4e98895fe7063094f592fd89964d49ba5a5c1da1f66b7169';

function view(cell: unknown) {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <CellInventoryLine cell={cell as Cell} />
    </QueryClientProvider>
  );
}

describe('CellInventoryLine (lane U, U6)', () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // The local API's answers for the user's example cell (2026-11-03).
    vi.mocked(api.getSporeObject).mockResolvedValue({
      sporeId: SPORE_ID,
      clusterId: CLUSTER_ID,
      mediaProfile: { tier: 'btc_ckb' },
    } as never);
    vi.mocked(api.getSporeCluster).mockResolvedValue({
      clusterId: CLUSTER_ID,
      name: 'Nervape Mask',
    } as never);
  });

  it('names a Spore cell as its collection and short id, with its composition chip and one bright link', async () => {
    view(spore);
    const line = await screen.findByTestId('cell-inventory');
    expect(await within(line).findByText(/Nervape Mask/)).toBeInTheDocument();
    expect(line).toHaveTextContent(/^This cell is Nervape Mask 0x7e3bc1bc…216539 BTC\+CKB$/);
    expect(line.querySelector('[data-class="objects"]')).not.toBeNull();
    expect(line.querySelector('[data-composition="btc_ckb"]')).not.toBeNull();
    const links = within(line).getAllByRole('link');
    expect(links).toHaveLength(1);
    expect(links[0]).toHaveAttribute('href', `/mainnet/objects/${SPORE_ID}`);
    expect(links[0]).toHaveClass('text-text-bright');
    expect(line).toHaveClass('text-section');
    // the cluster comes from the cell's own data: read in parallel with the object
    expect(api.getSporeCluster).toHaveBeenCalledWith(CLUSTER_ID);
    expect(api.getSporeObject).toHaveBeenCalledWith(SPORE_ID);
  });

  it('says only what it knows while the collection loads, and a read failure in place', async () => {
    vi.mocked(api.getSporeCluster).mockReturnValue(new Promise(() => {}));
    vi.mocked(api.getSporeObject).mockRejectedValue(new Error('node unavailable'));
    view(spore);
    const line = await screen.findByTestId('cell-inventory');
    expect(line).toHaveTextContent(/^This cell is a Spore object 0x7e3bc1bc…216539/);
    expect(await within(line).findByRole('alert')).toHaveTextContent(
      'could not read the object: node unavailable'
    );
  });

  it('says how much of which token an xUDT cell holds, from the cell itself', () => {
    view(xudt);
    const line = screen.getByTestId('cell-inventory');
    expect(line).toHaveTextContent(/^This cell holds 1,000 RGB\+\+$/);
    expect(line.querySelector('[data-class="tokens"]')).not.toBeNull();
    expect(within(line).getByRole('link', { name: '1,000 RGB++' })).toHaveAttribute(
      'href',
      '/mainnet/tokens/0xb5fdb4e8ad52f5e3ed4a294c01ca99f7811b06baef2e7fce5b7d4ebbf006717d'
    );
    // a token has no composition
    expect(line.querySelector('[data-composition]')).toBeNull();
  });

  // Lane B (B3): was the rouge "xudt cell without its decoded token amount" alert.
  it('says an xUDT cell with no amount holds no readable amount of its token, and why', () => {
    view(xudtNoAmount);
    const line = screen.getByTestId('cell-inventory');
    expect(line).toHaveTextContent(
      'This cell holds no readable amount of RGB++ · xUDT data is 0 bytes; an amount is its first 16 bytes (u128 LE)'
    );
    expect(within(line).getByRole('link', { name: 'RGB++' })).toHaveAttribute(
      'href',
      '/mainnet/tokens/0xb5fdb4e8ad52f5e3ed4a294c01ca99f7811b06baef2e7fce5b7d4ebbf006717d'
    );
    expect(screen.queryByTestId('cell-inventory-error')).toBeNull();
  });

  // Seals by kind (C8, C9): a token's unit is the token's own text, with no length bound. "5,913.45087427
  // LP-UTXOSwap V1" is 296 px, and beside the 88 px cell stone the line is 254 px at 390 and 224 px
  // at 360, so a whitespace-nowrap name scrolled the page sideways (C8). The name degrades in order
  // (C9): the pair on one line when it fits; else the amount whole on one line and the unit whole on
  // the next; and only a part wider than the line on its own wraps inside itself (spaces first, then
  // anywhere) — never at the unit's hyphen while the unit alone fits. So the pair is an atomic box
  // capped at the line, holding the amount and the unit as two atomic boxes capped at the line with
  // one space between them, the only break the pair offers. jsdom has no layout, so the classes that
  // make this are what is pinned; nothing is whitespace-nowrap (a nowrap part could not wrap even as
  // a last resort), and no box has a vertical-align of its own, so the last line sits on the
  // sentence's baseline.
  const LP_UNIT = 'LP-UTXOSwap V1';
  /** A part that stays whole whenever it fits a line and wraps inside itself only when it does not. */
  const expectPartBox = (part: Element, text: string) => {
    expect(part.tagName).toBe('SPAN');
    expect(part.textContent).toBe(text);
    expect(part.children).toHaveLength(0);
    expect(part).toHaveClass('inline-block', 'max-w-full', '[overflow-wrap:anywhere]');
    expect(part.className).not.toMatch(/(^|\s)align-/);
  };
  /** The link's one child is the name; nothing in it or around it is whitespace-nowrap. */
  const nameOf = (line: HTMLElement, text: string) => {
    const link = within(line).getByRole('link', { name: text });
    expect(link.children).toHaveLength(1);
    const name = link.firstElementChild as HTMLElement;
    expect(name.textContent).toBe(text);
    expect(name.closest('.whitespace-nowrap')).toBeNull();
    expect(name.querySelector('.whitespace-nowrap')).toBeNull();
    return name;
  };
  const lpCell = (token: Record<string, unknown>) => ({
    ...xudt,
    dataAnalysis: {
      ...xudt.dataAnalysis,
      token: { ...xudt.dataAnalysis.token, name: LP_UNIT, symbol: LP_UNIT, ...token },
    },
  });
  /** The pair: one capped box of the amount box, one space, the unit box. */
  const expectPair = (line: HTMLElement, amount: string, unit: string) => {
    const pair = nameOf(line, `${amount} ${unit}`);
    expect(pair.tagName).toBe('SPAN');
    expect(pair).toHaveClass('inline-block', 'max-w-full');
    expect(pair.className).not.toMatch(/(^|\s)align-/);
    const [first, space, second, ...rest] = Array.from(pair.childNodes);
    expect(rest).toEqual([]);
    expectPartBox(first as Element, amount);
    expect(space.nodeType).toBe(Node.TEXT_NODE);
    expect(space.textContent).toBe(' ');
    expectPartBox(second as Element, unit);
  };

  it('breaks a token’s amount from its unbounded unit before it breaks inside either, never past the line', () => {
    view(lpCell({ rawAmount: '591345087427', formattedAmount: '5913.45087427' }));
    const line = screen.getByTestId('cell-inventory');
    expect(line).toHaveTextContent(/^This cell holds 5,913\.45087427 LP-UTXOSwap V1$/);
    expectPair(line, '5,913.45087427', LP_UNIT);
  });

  // An amount has no length bound either (0 decimals, a huge supply), and a token with unknown
  // decimals reads "… (raw)": the amount's own space stays inside the amount's box.
  it('keeps a raw amount with its marker in the amount’s box, the unit in its own', () => {
    view(lpCell({ decimals: null, rawAmount: '591345087427', formattedAmount: null }));
    const line = screen.getByTestId('cell-inventory');
    expect(line).toHaveTextContent(/^This cell holds 591,345,087,427 \(raw\) LP-UTXOSwap V1$/);
    expectPair(line, '591,345,087,427 (raw)', LP_UNIT);
  });

  it('caps an unbounded unit with no readable amount at the line the same way', () => {
    view({
      ...xudtNoAmount,
      dataAnalysis: {
        ...xudtNoAmount.dataAnalysis,
        token: { ...xudtNoAmount.dataAnalysis.token, name: LP_UNIT, symbol: LP_UNIT },
      },
    });
    const line = screen.getByTestId('cell-inventory');
    expect(line).toHaveTextContent(/^This cell holds no readable amount of LP-UTXOSwap V1 · /);
    expectPartBox(nameOf(line, LP_UNIT), LP_UNIT);
  });

  it('names a DAO deposit and a withdrawal request with their exact CKB', () => {
    const deposit = view(dao);
    const line = screen.getByTestId('cell-inventory');
    expect(line).toHaveTextContent(/^This cell is a Nervos DAO deposit of 12,500(\.0+)? CKB$/);
    expect(within(line).getByRole('link', { name: 'Nervos DAO deposit' })).toHaveAttribute(
      'href',
      '/mainnet/dao'
    );
    expect(line.querySelector('[data-class="dao"]')).not.toBeNull();
    deposit.unmount();
    view(daoWithdraw);
    expect(screen.getByTestId('cell-inventory')).toHaveTextContent(
      /^This cell is a Nervos DAO withdrawal request for 9,200(\.0+)? CKB$/
    );
  });

  it('names a .bit cell from its own data and a did:ckb as its page does', async () => {
    const first = view(dotbit);
    const bit = screen.getByTestId('cell-inventory');
    expect(bit).toHaveTextContent(/^This cell is conaservices\.bit Pure CKB$/);
    expect(within(bit).getByRole('link')).toHaveAttribute(
      'href',
      '/mainnet/identities/dotbit/0x000063c0e5d40b236bb9b6b7b5831d87e4429d5c'
    );
    expect(api.getItemProtocol).not.toHaveBeenCalled();
    first.unmount();
    vi.mocked(api.getItemProtocol).mockResolvedValue({
      kind: 'did_ckb',
      itemId: did.type.args,
      versions: [
        {
          decoded: {
            did: 'did:ckb:almulti2h2t75bci2u6spnnslvav36s7',
            document: { alsoKnownAs: ['at://3333.web5.bbs.fans'] },
          },
        },
      ],
    } as never);
    view(did);
    const line = await screen.findByText('3333.web5.bbs.fans');
    expect(line.closest('[data-testid="cell-inventory"]')).toHaveTextContent(
      /^This cell is 3333\.web5\.bbs\.fans Pure CKB$/
    );
    expect(api.getItemProtocol).toHaveBeenCalledWith('did', did.type.args);
  });

  it('names an m-NFT cell by its class and token index', async () => {
    vi.mocked(api.getMnftItemDetail).mockResolvedValue({
      nftId: mnft.type.args,
      tokenIndex: 0,
      class: { name: 'Non-Fungible Santa Claus' },
      composition: { tier: 'centralized_mixture' },
    } as never);
    view(mnft);
    expect(await screen.findByText('Non-Fungible Santa Claus #0')).toHaveAttribute(
      'href',
      `/mainnet/objects/mnft/${mnft.type.args}`
    );
    expect(
      screen.getByTestId('cell-inventory').querySelector('[data-composition="centralized_mixture"]')
    ).not.toBeNull();
  });

  it('renders no line for a plain cell or an unhandled protocol', () => {
    const { container } = view({ ...spore, protocolScript: { lock: null, type: null } });
    expect(container).toBeEmptyDOMElement();
    const other = view({ ...spore, protocolScript: { lock: null, type: 'ckbfs' } });
    expect(other.container).toBeEmptyDOMElement();
    expect(api.getSporeObject).not.toHaveBeenCalled();
  });

  it('says in place, never a guess, when a handled cell does not name its item', () => {
    view({ ...xudt, dataAnalysis: undefined });
    expect(screen.queryByTestId('cell-inventory')).toBeNull();
    expect(screen.getByTestId('cell-inventory-error')).toHaveTextContent(
      'xudt cell without its decoded token amount'
    );
  });
});
