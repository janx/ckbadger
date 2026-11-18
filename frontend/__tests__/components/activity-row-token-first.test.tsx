// Readability 2026-11-18, lane B4 (pG), item 4: on a token's page the token's amount leads its
// participant line and the CKB follows. The review found the page's own token fourth on its lines
// ("address · lock · −2,764.11 CKB · RGB++ amount · occupied"), after the CKB it costs to hold it.
// The row learns which token the page is about from the token page's feed (AssetFeed's `token`),
// the one place that knows it.
import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import { seededAddress } from '@/__tests__/utils/ckb-address';
import { AssetFeed } from '@/components/asset/asset-feed';
import type { GlobalActivity, ItemDelta, ParticipantInfo } from '@/lib/api';

const PAGE_TOKEN = `0x${'77'.repeat(32)}`;
const OTHER_TOKEN = `0x${'88'.repeat(32)}`;
const token = (hash: string, symbol: string, delta: string): ItemDelta => ({
  kind: 'token',
  typeScriptHash: hash,
  delta,
  symbol,
  decimals: 8,
});

function party(seed: string, ckbDelta: string, itemDeltas: ItemDelta[]): ParticipantInfo {
  return {
    address: seededAddress(seed),
    lockHash: `0x${seed.padStart(64, '0')}`,
    lockHashPrefix: null,
    lockKind: 'Default Lock',
    roles: [],
    tags: itemDeltas.length ? 1 : 0,
    ckbDelta,
    usedDelta: '0',
    itemDeltas,
  };
}

const swap: GlobalActivity = {
  txHash: `0x${'5a'.repeat(32)}`,
  blockNumber: 20724160,
  txIndex: 2,
  timestamp: '1761174000000',
  isCellbase: false,
  protocolActions: [],
  typeCalls: [],
  lockCalls: [],
  participants: [
    // pays the page's token and another, and the CKB
    party('payer1', '-276411026713', [
      token(OTHER_TOKEN, 'LP', '-591345087427'),
      token(PAGE_TOKEN, 'RGB++', '-1300019363874'),
    ]),
    party('recvr1', '276411017683', [token(PAGE_TOKEN, 'RGB++', '1300019363874')]),
    // moves no token: its CKB stays first
    party('feepay', '-9030', []),
  ],
};

function feed(tokenPage: boolean) {
  return render(
    <AssetFeed
      queryKey={`token-first-${tokenPage}`}
      scope="Every transaction that moved RGB++"
      plain
      mark="tokens"
      token={tokenPage ? { typeHash: PAGE_TOKEN, symbol: 'RGB++', decimals: 8 } : undefined}
      loadActivities={async () => ({
        data: [swap],
        total: 1,
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })}
      loadTransactions={async () => ({
        data: [],
        total: 0,
        limit: 50,
        hasMore: false,
        nextCursor: null,
      })}
    />
  );
}

/** Each part of a participant line, in order: its role, and for an item the token it moved. */
function parts(line: Element): string[] {
  return [...line.children].map((e) => {
    const role = e.getAttribute('data-role');
    if (role !== 'item') return role ?? '?';
    const link = e.querySelector('a[href*="/tokens/"]');
    return `item:${link?.getAttribute('href')?.split('/tokens/')[1] === PAGE_TOKEN ? 'page' : 'other'}`;
  });
}

describe('on a token’s page the token’s amount leads its line (B4 item 4)', () => {
  it('draws the page’s token before the CKB; any other token after it, as before', async () => {
    feed(true);
    await screen.findByRole('article');
    // the lines in the row's order: what was paid, largest first, then what was received
    const lines = [...document.querySelectorAll('[data-role="participant"]')];
    expect(lines.map(parts)).toEqual([
      ['party', 'item:page', 'ckb', 'item:other'],
      ['party', 'ckb'],
      ['party', 'item:page', 'ckb'],
    ]);
  });

  it('leaves the CKB first on a feed that is about no token', async () => {
    feed(false);
    await screen.findByRole('article');
    const lines = [...document.querySelectorAll('[data-role="participant"]')];
    expect(lines.map(parts)).toEqual([
      ['party', 'ckb', 'item:other', 'item:page'],
      ['party', 'ckb'],
      ['party', 'ckb', 'item:page'],
    ]);
  });
});
