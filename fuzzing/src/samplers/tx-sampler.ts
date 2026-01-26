import { ckbadgerApi } from '../fetchers/ckbadger';
import { BlockSampler } from './block-sampler';

export class TxSampler {
  private blockSampler: BlockSampler;

  constructor() {
    this.blockSampler = new BlockSampler();
  }

  async sampleFromBlocks(blockNumbers: number[], countPerBlock: number): Promise<string[]> {
    const txHashes: string[] = [];

    for (const blockNum of blockNumbers) {
      try {
        const txs = await ckbadgerApi.getTransactions({
          blockNumber: blockNum,
          limit: countPerBlock,
        });
        txHashes.push(...txs.data.map((tx) => tx.hash));
      } catch {
        continue;
      }
    }

    return txHashes;
  }

  async sampleRandom(count: number): Promise<string[]> {
    const blocksToSample = Math.ceil(count / 3);
    const blockNumbers = await this.blockSampler.sampleRandom(blocksToSample);
    const txHashes = await this.sampleFromBlocks(blockNumbers, 5);
    return this.shuffleAndTake(txHashes, count);
  }

  async sampleRecent(count: number): Promise<string[]> {
    const txs = await ckbadgerApi.getTransactions({ limit: count });
    return txs.data.map((tx) => tx.hash);
  }

  private shuffleAndTake<T>(array: T[], count: number): T[] {
    const shuffled = [...array];
    for (let i = shuffled.length - 1; i > 0; i--) {
      const j = Math.floor(Math.random() * (i + 1));
      [shuffled[i], shuffled[j]] = [shuffled[j], shuffled[i]];
    }
    return shuffled.slice(0, count);
  }
}
