import { test, expect, Page } from '@playwright/test';

const BASE_URL = process.env.CKBADGER_FRONTEND_URL || 'http://localhost:3000';

interface ScrapedData {
  displayedCounts: Record<string, number>;
  listCounts: Record<string, number>;
}

async function extractNumber(text: string | null): Promise<number | null> {
  if (!text) return null;
  const match = text.match(/\d[\d,]*/);
  if (!match) return null;
  return parseInt(match[0].replace(/,/g, ''), 10);
}

test.describe('Block Page Visual Consistency', () => {
  test('transactions count matches list on block detail page', async ({ page }) => {
    await page.goto(`${BASE_URL}/blocks/1000000`);
    await page.waitForLoadState('networkidle');

    const txTabText = await page
      .locator('button, [role="tab"]')
      .filter({ hasText: /Transactions/i })
      .first()
      .textContent();
    const displayedCount = await extractNumber(txTabText);

    const txRows = await page.locator('table tbody tr, [data-testid="tx-row"]').count();

    if (displayedCount !== null) {
      expect(txRows).toBe(displayedCount);
    }
  });

  test('proposals count matches list on block detail page', async ({ page }) => {
    await page.goto(`${BASE_URL}/blocks/1000000`);
    await page.waitForLoadState('networkidle');

    const proposalsTab = page
      .locator('button, [role="tab"]')
      .filter({ hasText: /Proposals/i })
      .first();
    const proposalsTabText = await proposalsTab.textContent();
    const displayedCount = await extractNumber(proposalsTabText);

    if (displayedCount && displayedCount > 0) {
      await proposalsTab.click();
      await page.waitForTimeout(500);

      const proposalRows = await page
        .locator('[data-testid="proposal-row"], .proposal-item')
        .count();

      if (proposalRows > 0) {
        expect(proposalRows).toBe(displayedCount);
      }
    }
  });
});

test.describe('Transaction Page Visual Consistency', () => {
  test('inputs/outputs count matches displayed items', async ({ page }) => {
    const txsResponse = await fetch(
      `${process.env.CKBADGER_API_URL || 'http://localhost:3001/api/v1'}/transactions?limit=1`
    );
    const txsData = await txsResponse.json();
    const sampleTxHash = txsData.data?.[0]?.hash;

    if (!sampleTxHash) {
      test.skip();
      return;
    }

    await page.goto(`${BASE_URL}/tx/${sampleTxHash}`);
    await page.waitForLoadState('networkidle');

    const inputsHeader = page
      .locator('h2, h3, .section-header')
      .filter({ hasText: /Inputs/i })
      .first();
    const inputsHeaderText = await inputsHeader.textContent();
    const displayedInputsCount = await extractNumber(inputsHeaderText);

    const inputItems = await page
      .locator('[data-testid="input-item"], .input-cell, .cell-input')
      .count();

    if (displayedInputsCount !== null && inputItems > 0) {
      expect(inputItems).toBe(displayedInputsCount);
    }
  });
});

test.describe('Address Page Visual Consistency', () => {
  test('live cells count matches tab label', async ({ page }) => {
    const addressesResponse = await fetch(
      `${process.env.CKBADGER_API_URL || 'http://localhost:3001/api/v1'}/addresses/top?limit=1`
    );
    const addressesData = await addressesResponse.json();
    const sampleAddress = addressesData?.[0]?.address;

    if (!sampleAddress) {
      test.skip();
      return;
    }

    await page.goto(`${BASE_URL}/address/${sampleAddress}`);
    await page.waitForLoadState('networkidle');

    const cellsTabText = await page
      .locator('button, [role="tab"]')
      .filter({ hasText: /Cells|Live Cells/i })
      .first()
      .textContent();
    const displayedCellsCount = await extractNumber(cellsTabText);

    if (displayedCellsCount !== null) {
      const cellsCard = page
        .locator('.stat-card, [data-testid="live-cells-stat"]')
        .filter({ hasText: /Live Cells/i });
      const cardValue = await cellsCard.locator('.stat-value, .value').textContent();
      const cardCount = await extractNumber(cardValue);

      if (cardCount !== null) {
        expect(displayedCellsCount).toBe(cardCount);
      }
    }
  });
});

test.describe('Token Page Visual Consistency', () => {
  test('holders count matches displayed stat', async ({ page }) => {
    const tokensResponse = await fetch(
      `${process.env.CKBADGER_API_URL || 'http://localhost:3001/api/v1'}/tokens?limit=1`
    );
    const tokensData = await tokensResponse.json();
    const sampleToken = tokensData.data?.[0];

    if (!sampleToken) {
      test.skip();
      return;
    }

    await page.goto(`${BASE_URL}/tokens/${sampleToken.typeScriptHash}`);
    await page.waitForLoadState('networkidle');

    const holdersTabText = await page
      .locator('button, [role="tab"]')
      .filter({ hasText: /Holders/i })
      .first()
      .textContent();
    const tabCount = await extractNumber(holdersTabText);

    const holdersStatCard = page
      .locator('.stat-card, [data-testid="holders-stat"]')
      .filter({ hasText: /Holders/i });
    const statValue = await holdersStatCard.locator('.stat-value, .value').textContent();
    const statCount = await extractNumber(statValue);

    if (tabCount !== null && statCount !== null) {
      expect(tabCount).toBe(statCount);
    }
  });
});

test.describe('Homepage Consistency', () => {
  test('latest blocks count matches network stat', async ({ page }) => {
    await page.goto(BASE_URL);
    await page.waitForLoadState('networkidle');

    const latestBlockStat = await page
      .locator('.stat-card, [data-testid="latest-block"]')
      .filter({ hasText: /Latest Block|Block Height/i })
      .first()
      .textContent();
    const displayedLatestBlock = await extractNumber(latestBlockStat);

    const firstBlockLink = page.locator('a[href*="/blocks/"]').first();
    const firstBlockText = await firstBlockLink.textContent();
    const firstBlockNumber = await extractNumber(firstBlockText);

    if (displayedLatestBlock !== null && firstBlockNumber !== null) {
      const diff = Math.abs(displayedLatestBlock - firstBlockNumber);
      expect(diff).toBeLessThan(10);
    }
  });
});
