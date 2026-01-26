import { ckbadgerApi } from '../fetchers/ckbadger';
import type { PageConsistencyCheck } from '../types';

export class PageConsistencyChecker {
  async checkBlockPage(blockNumber: number): Promise<PageConsistencyCheck[]> {
    const checks: PageConsistencyCheck[] = [];

    const block = await ckbadgerApi.getBlock(blockNumber);
    const txs = await ckbadgerApi.getTransactions({ blockNumber, limit: 500 });

    const actualTxCount = txs.data.length;
    const isConsistent =
      block.transactionsCount === actualTxCount ||
      (txs.hasMore && actualTxCount >= block.transactionsCount);

    checks.push({
      page: `/blocks/${blockNumber}`,
      countField: 'block.transactionsCount',
      countValue: block.transactionsCount,
      listLength: actualTxCount,
      isConsistent,
      details: txs.hasMore ? `Has more (fetched ${actualTxCount})` : undefined,
    });

    if (block.proposalsCount > 0) {
      const proposals = await ckbadgerApi.getBlockProposals(blockNumber);
      checks.push({
        page: `/blocks/${blockNumber}`,
        countField: 'block.proposalsCount',
        countValue: block.proposalsCount,
        listLength: proposals.length,
        isConsistent: block.proposalsCount === proposals.length,
      });
    }

    return checks;
  }

  async checkTransactionPage(txHash: string): Promise<PageConsistencyCheck[]> {
    const checks: PageConsistencyCheck[] = [];

    const tx = await ckbadgerApi.getTransaction(txHash);
    const detail = await ckbadgerApi.getTransactionDetail(txHash);

    const actualInputsCount = detail.inputs?.length ?? 0;
    const isCellbase = tx.isCellbase;

    checks.push({
      page: `/tx/${txHash}`,
      countField: 'tx.inputsCount',
      countValue: tx.inputsCount,
      listLength: actualInputsCount,
      isConsistent:
        tx.inputsCount === actualInputsCount ||
        (isCellbase && tx.inputsCount === 1 && actualInputsCount === 0),
      details: isCellbase ? 'Cellbase transaction (no real inputs)' : undefined,
    });

    const actualOutputsCount = detail.outputs?.length ?? 0;
    checks.push({
      page: `/tx/${txHash}`,
      countField: 'tx.outputsCount',
      countValue: tx.outputsCount,
      listLength: actualOutputsCount,
      isConsistent: tx.outputsCount === actualOutputsCount,
    });

    return checks;
  }

  async checkAddressPage(address: string): Promise<PageConsistencyCheck[]> {
    const checks: PageConsistencyCheck[] = [];

    const addressInfo = await ckbadgerApi.getAddress(address);

    const liveCells = await ckbadgerApi.getLiveCells({
      lockScriptHash: addressInfo.lockScriptHash,
      limit: 1,
    });

    checks.push({
      page: `/address/${address}`,
      countField: 'address.liveCellsCount',
      countValue: addressInfo.liveCellsCount,
      listLength: liveCells.total,
      isConsistent: addressInfo.liveCellsCount === liveCells.total,
    });

    const txs = await ckbadgerApi.getAddressTransactions(address, { limit: 1 });
    checks.push({
      page: `/address/${address}`,
      countField: 'address.transactionsCount',
      countValue: addressInfo.transactionsCount,
      listLength: txs.total,
      isConsistent: addressInfo.transactionsCount === txs.total,
    });

    return checks;
  }

  async checkTokenPage(typeHash: string): Promise<PageConsistencyCheck[]> {
    const checks: PageConsistencyCheck[] = [];

    const token = await ckbadgerApi.getToken(typeHash);

    const holders = await ckbadgerApi.getTokenHolders(typeHash, { limit: 1 });
    checks.push({
      page: `/tokens/${typeHash}`,
      countField: 'token.holdersCount',
      countValue: token.holdersCount,
      listLength: holders.total,
      isConsistent: token.holdersCount === holders.total,
    });

    const transfers = await ckbadgerApi.getTokenTransfers(typeHash, { limit: 1 });
    checks.push({
      page: `/tokens/${typeHash}`,
      countField: 'token.transfersCount',
      countValue: token.transfersCount,
      listLength: transfers.total,
      isConsistent: token.transfersCount === transfers.total,
    });

    return checks;
  }

  async checkDaoPage(): Promise<PageConsistencyCheck[]> {
    const checks: PageConsistencyCheck[] = [];

    const stats = await ckbadgerApi.getDaoStatistics();
    const allDeposits = await ckbadgerApi.getDaoDeposits({ limit: 1 });

    const activeCountSanity = stats.activeDeposits <= allDeposits.total;

    checks.push({
      page: '/dao',
      countField: 'stats.activeDeposits (sanity: active <= total)',
      countValue: stats.activeDeposits,
      listLength: allDeposits.total,
      isConsistent: activeCountSanity,
      details: `Active: ${stats.activeDeposits}, Total: ${allDeposits.total}`,
    });

    return checks;
  }
}
