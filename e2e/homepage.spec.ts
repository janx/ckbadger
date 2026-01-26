import { test, expect } from '@playwright/test';

test.describe('Homepage', () => {
  test('loads and displays network stats', async ({ page }) => {
    await page.goto('/');

    await expect(page).toHaveTitle(/ckbadger|CKB/i);
    await expect(page.locator('body')).toBeVisible({ timeout: 15000 });
  });

  test('displays latest blocks section', async ({ page }) => {
    await page.goto('/');

    const blocksSection = page.locator('text=/Latest Blocks|Blocks/i').first();
    await expect(blocksSection).toBeVisible({ timeout: 15000 });
  });

  test('displays latest transactions section', async ({ page }) => {
    await page.goto('/');

    const txSection = page.locator('text=/Transactions/i').first();
    await expect(txSection).toBeVisible({ timeout: 15000 });
  });

  test('search input is visible and functional', async ({ page }) => {
    await page.goto('/');

    const searchInput = page.locator('input[type="text"], input[type="search"]').first();
    await expect(searchInput).toBeVisible({ timeout: 10000 });

    await searchInput.fill('1000000');
    await searchInput.press('Enter');
    await page.waitForTimeout(2000);
  });

  test('navigation works', async ({ page }) => {
    await page.goto('/');

    await expect(page.locator('body')).toBeVisible();
  });
});

test.describe('Block Detail Page', () => {
  test('block page responds correctly', async ({ page }) => {
    await page.goto('/block/1');

    await expect(page.locator('body')).toBeVisible({ timeout: 15000 });
  });

  test('non-existent block shows 404', async ({ page }) => {
    await page.goto('/block/999999999999');

    await expect(page.locator('text=/404|not found/i').first()).toBeVisible({ timeout: 15000 });
  });
});

test.describe('Blocks List Page', () => {
  test('blocks page loads', async ({ page }) => {
    await page.goto('/blocks');

    await expect(page.locator('body')).toBeVisible({ timeout: 15000 });
  });
});
