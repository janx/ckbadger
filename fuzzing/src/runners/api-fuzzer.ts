import { config, defaultFuzzerOptions } from '../config';
import { ckbadgerApi, officialApi } from '../fetchers';
import { BlockSampler, TxSampler, AddressSampler } from '../samplers';
import { BlockComparator, TxComparator, AddressComparator } from '../comparators';
import { InconsistencyReporter } from '../reporters';
import type { ComparisonResult, FuzzerOptions } from '../types';

export class ApiFuzzer {
  private options: FuzzerOptions;
  private blockSampler: BlockSampler;
  private txSampler: TxSampler;
  private addressSampler: AddressSampler;
  private blockComparator: BlockComparator;
  private txComparator: TxComparator;
  private addressComparator: AddressComparator;
  private reporter: InconsistencyReporter;

  constructor(options: Partial<FuzzerOptions> = {}) {
    this.options = { ...defaultFuzzerOptions, ...options };
    this.blockSampler = new BlockSampler();
    this.txSampler = new TxSampler();
    this.addressSampler = new AddressSampler();
    this.blockComparator = new BlockComparator();
    this.txComparator = new TxComparator();
    this.addressComparator = new AddressComparator();
    this.reporter = new InconsistencyReporter(this.options.outputDir);
  }

  async run(): Promise<void> {
    const startTime = new Date();
    const allIssues: ComparisonResult[] = [];

    console.log('\n🔍 Starting API Fuzzing...\n');
    console.log(`Ckbadger API: ${config.ckbadger.baseUrl}`);
    console.log(`Official API: ${config.official.baseUrl}`);
    console.log(`Block samples: ${this.options.blockSampleSize}`);
    console.log(`TX samples: ${this.options.txSampleSize}`);
    console.log(`Address samples: ${this.options.addressSampleSize}`);

    const blockIssues = await this.fuzzBlocks();
    allIssues.push(...blockIssues);

    const txIssues = await this.fuzzTransactions();
    allIssues.push(...txIssues);

    const addressIssues = await this.fuzzAddresses();
    allIssues.push(...addressIssues);

    const report = this.reporter.generateReport(allIssues, [], 'api', startTime);
    this.reporter.printToConsole(report);
    this.reporter.saveToFile(report);
  }

  private async fuzzBlocks(): Promise<ComparisonResult[]> {
    console.log(`\n📦 Sampling ${this.options.blockSampleSize} random blocks...`);

    const issues: ComparisonResult[] = [];
    const blockNumbers = await this.blockSampler.sampleRandom(this.options.blockSampleSize);

    let completed = 0;
    for (const blockNum of blockNumbers) {
      try {
        const [ourBlock, theirBlock] = await Promise.all([
          ckbadgerApi.getBlock(blockNum),
          officialApi.getBlock(blockNum),
        ]);

        const blockIssues = this.blockComparator.compare(ourBlock, theirBlock);
        issues.push(...blockIssues);

        if (this.options.verbose && blockIssues.length > 0) {
          console.log(`  Block #${blockNum}: ${blockIssues.length} issue(s)`);
        }
      } catch (error) {
        if (this.options.continueOnError) {
          issues.push({
            entity: 'block',
            id: String(blockNum),
            field: 'fetch',
            ckbadger: 'error',
            official: 'N/A',
            severity: 'critical',
            message: `Failed to fetch block #${blockNum}: ${(error as Error).message}`,
          });
        } else {
          throw error;
        }
      }

      completed++;
      if (completed % 10 === 0) {
        process.stdout.write(`  Progress: ${completed}/${blockNumbers.length}\r`);
      }
    }

    console.log(`  ✓ Blocks checked: ${blockNumbers.length}, Issues: ${issues.length}`);
    return issues;
  }

  private async fuzzTransactions(): Promise<ComparisonResult[]> {
    console.log(`\n📝 Sampling ${this.options.txSampleSize} random transactions...`);

    const issues: ComparisonResult[] = [];
    const txHashes = await this.txSampler.sampleRandom(this.options.txSampleSize);

    let completed = 0;
    for (const hash of txHashes) {
      try {
        const [ourTx, theirTx] = await Promise.all([
          ckbadgerApi.getTransactionDetail(hash),
          officialApi.getTransaction(hash),
        ]);

        const txIssues = this.txComparator.compare(ourTx, theirTx);
        issues.push(...txIssues);

        if (this.options.verbose && txIssues.length > 0) {
          console.log(`  TX ${hash.slice(0, 10)}...: ${txIssues.length} issue(s)`);
        }
      } catch (error) {
        if (this.options.continueOnError) {
          issues.push({
            entity: 'transaction',
            id: hash,
            field: 'fetch',
            ckbadger: 'error',
            official: 'N/A',
            severity: 'critical',
            message: `Failed to fetch TX ${hash.slice(0, 16)}...: ${(error as Error).message}`,
          });
        } else {
          throw error;
        }
      }

      completed++;
      if (completed % 10 === 0) {
        process.stdout.write(`  Progress: ${completed}/${txHashes.length}\r`);
      }
    }

    console.log(`  ✓ Transactions checked: ${txHashes.length}, Issues: ${issues.length}`);
    return issues;
  }

  private async fuzzAddresses(): Promise<ComparisonResult[]> {
    console.log(`\n👤 Sampling ${this.options.addressSampleSize} addresses...`);

    const issues: ComparisonResult[] = [];
    const addresses = await this.addressSampler.sampleMixed(this.options.addressSampleSize);

    let completed = 0;
    for (const addr of addresses) {
      try {
        const [ourAddr, theirAddr] = await Promise.all([
          ckbadgerApi.getAddress(addr),
          officialApi.getAddress(addr),
        ]);

        const addrIssues = this.addressComparator.compare(ourAddr, theirAddr);
        issues.push(...addrIssues);

        if (this.options.verbose && addrIssues.length > 0) {
          console.log(`  Address ${addr.slice(0, 12)}...: ${addrIssues.length} issue(s)`);
        }
      } catch (error) {
        if (this.options.continueOnError) {
          issues.push({
            entity: 'address',
            id: addr,
            field: 'fetch',
            ckbadger: 'error',
            official: 'N/A',
            severity: 'critical',
            message: `Failed to fetch address: ${(error as Error).message}`,
          });
        } else {
          throw error;
        }
      }

      completed++;
      if (completed % 5 === 0) {
        process.stdout.write(`  Progress: ${completed}/${addresses.length}\r`);
      }
    }

    console.log(`  ✓ Addresses checked: ${addresses.length}, Issues: ${issues.length}`);
    return issues;
  }
}
