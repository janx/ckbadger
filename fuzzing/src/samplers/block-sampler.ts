import { config } from '../config';
import { ckbadgerApi } from '../fetchers/ckbadger';
import type { SamplingStrategy } from '../types';

export class BlockSampler {
  private strategy: SamplingStrategy;

  constructor(strategy?: SamplingStrategy) {
    this.strategy = strategy ?? config.sampling.defaultStrategy;
  }

  async sampleRandom(count: number): Promise<number[]> {
    const stats = await ckbadgerApi.getNetworkStats();
    const tipBlock = stats.latestBlock;

    const samples: number[] = [];
    const seen = new Set<number>();

    while (samples.length < count) {
      const blockNum = this.generateBlockNumber(tipBlock);

      if (!seen.has(blockNum) && blockNum >= 0) {
        seen.add(blockNum);
        samples.push(blockNum);
      }
    }

    return samples.sort((a, b) => b - a);
  }

  async sampleRecent(count: number): Promise<number[]> {
    const stats = await ckbadgerApi.getNetworkStats();
    const tipBlock = stats.latestBlock;

    const samples: number[] = [];
    for (let i = 0; i < count; i++) {
      samples.push(tipBlock - i);
    }
    return samples;
  }

  async sampleRange(start: number, end: number, count: number): Promise<number[]> {
    const range = end - start;
    const samples: number[] = [];
    const seen = new Set<number>();

    while (samples.length < count && samples.length < range) {
      const blockNum = start + Math.floor(Math.random() * range);
      if (!seen.has(blockNum)) {
        seen.add(blockNum);
        samples.push(blockNum);
      }
    }

    return samples.sort((a, b) => b - a);
  }

  private generateBlockNumber(tipBlock: number): number {
    const rand = Math.random();

    if (rand < this.strategy.recentWeight) {
      const range = Math.min(config.sampling.recentBlocksRange, tipBlock);
      return tipBlock - Math.floor(Math.random() * range);
    }

    if (rand < this.strategy.recentWeight + this.strategy.midRangeWeight) {
      const start = config.sampling.midRangeStart;
      const end = Math.min(
        config.sampling.midRangeEnd,
        tipBlock - config.sampling.recentBlocksRange
      );
      if (end > start) {
        return start + Math.floor(Math.random() * (end - start));
      }
    }

    return Math.floor(Math.random() * Math.min(config.sampling.genesisRange, tipBlock));
  }
}
