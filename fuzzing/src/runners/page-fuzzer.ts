import { defaultFuzzerOptions } from '../config';
import { BlockSampler, TxSampler, AddressSampler } from '../samplers';
import { PageConsistencyChecker } from '../comparators';
import { InconsistencyReporter } from '../reporters';
import { ckbadgerApi } from '../fetchers';
import type { PageConsistencyCheck, FuzzerOptions } from '../types';

export class PageFuzzer {
  private options: FuzzerOptions;
  private blockSampler: BlockSampler;
  private txSampler: TxSampler;
  private addressSampler: AddressSampler;
  private checker: PageConsistencyChecker;
  private reporter: InconsistencyReporter;

  constructor(options: Partial<FuzzerOptions> = {}) {
    this.options = { ...defaultFuzzerOptions, ...options };
    this.blockSampler = new BlockSampler();
    this.txSampler = new TxSampler();
    this.addressSampler = new AddressSampler();
    this.checker = new PageConsistencyChecker();
    this.reporter = new InconsistencyReporter(this.options.outputDir);
  }

  async run(): Promise<void> {
    const startTime = new Date();
    const allIssues: PageConsistencyCheck[] = [];

    console.log('\n🔍 Starting Page Consistency Fuzzing...\n');

    const blockIssues = await this.checkBlockPages();
    allIssues.push(...blockIssues);

    const txIssues = await this.checkTransactionPages();
    allIssues.push(...txIssues);

    const addressIssues = await this.checkAddressPages();
    allIssues.push(...addressIssues);

    const tokenIssues = await this.checkTokenPages();
    allIssues.push(...tokenIssues);

    const daoIssues = await this.checkDaoPage();
    allIssues.push(...daoIssues);

    const report = this.reporter.generateReport([], allIssues, 'page', startTime);
    this.reporter.printToConsole(report);
    this.reporter.saveToFile(report);
  }

  private async checkBlockPages(): Promise<PageConsistencyCheck[]> {
    console.log(`📦 Checking ${this.options.blockSampleSize} block pages...`);

    const allChecks: PageConsistencyCheck[] = [];
    const blockNumbers = await this.blockSampler.sampleRandom(this.options.blockSampleSize);

    for (const blockNum of blockNumbers) {
      try {
        const checks = await this.checker.checkBlockPage(blockNum);
        const inconsistent = checks.filter((c) => !c.isConsistent);
        allChecks.push(...inconsistent);

        if (this.options.verbose && inconsistent.length > 0) {
          console.log(`  Block #${blockNum}: ${inconsistent.length} inconsistency`);
        }
      } catch (error) {
        if (!this.options.continueOnError) throw error;
        if (this.options.verbose) {
          console.log(`  Block #${blockNum}: fetch error`);
        }
      }
    }

    const inconsistentCount = allChecks.length;
    console.log(
      `  ✓ Block pages checked: ${blockNumbers.length}, Inconsistencies: ${inconsistentCount}`
    );
    return allChecks;
  }

  private async checkTransactionPages(): Promise<PageConsistencyCheck[]> {
    console.log(`📝 Checking ${this.options.txSampleSize} transaction pages...`);

    const allChecks: PageConsistencyCheck[] = [];
    const txHashes = await this.txSampler.sampleRandom(this.options.txSampleSize);

    for (const hash of txHashes) {
      try {
        const checks = await this.checker.checkTransactionPage(hash);
        const inconsistent = checks.filter((c) => !c.isConsistent);
        allChecks.push(...inconsistent);

        if (this.options.verbose && inconsistent.length > 0) {
          console.log(`  TX ${hash.slice(0, 10)}...: ${inconsistent.length} inconsistency`);
        }
      } catch (error) {
        if (!this.options.continueOnError) throw error;
      }
    }

    const inconsistentCount = allChecks.length;
    console.log(`  ✓ TX pages checked: ${txHashes.length}, Inconsistencies: ${inconsistentCount}`);
    return allChecks;
  }

  private async checkAddressPages(): Promise<PageConsistencyCheck[]> {
    console.log(`👤 Checking ${this.options.addressSampleSize} address pages...`);

    const allChecks: PageConsistencyCheck[] = [];
    const addresses = await this.addressSampler.sampleMixed(this.options.addressSampleSize);

    for (const addr of addresses) {
      try {
        const checks = await this.checker.checkAddressPage(addr);
        const inconsistent = checks.filter((c) => !c.isConsistent);
        allChecks.push(...inconsistent);

        if (this.options.verbose && inconsistent.length > 0) {
          console.log(`  Address ${addr.slice(0, 12)}...: ${inconsistent.length} inconsistency`);
        }
      } catch (error) {
        if (!this.options.continueOnError) throw error;
      }
    }

    const inconsistentCount = allChecks.length;
    console.log(
      `  ✓ Address pages checked: ${addresses.length}, Inconsistencies: ${inconsistentCount}`
    );
    return allChecks;
  }

  private async checkTokenPages(): Promise<PageConsistencyCheck[]> {
    console.log(`🪙 Checking token pages...`);

    const allChecks: PageConsistencyCheck[] = [];

    try {
      const tokens = await ckbadgerApi.getTokens({ limit: 10 });

      for (const token of tokens.data) {
        try {
          const checks = await this.checker.checkTokenPage(token.typeScriptHash);
          const inconsistent = checks.filter((c) => !c.isConsistent);
          allChecks.push(...inconsistent);

          if (this.options.verbose && inconsistent.length > 0) {
            console.log(
              `  Token ${token.symbol ?? token.typeScriptHash.slice(0, 10)}...: ${inconsistent.length} inconsistency`
            );
          }
        } catch (error) {
          if (!this.options.continueOnError) throw error;
        }
      }

      console.log(
        `  ✓ Token pages checked: ${tokens.data.length}, Inconsistencies: ${allChecks.length}`
      );
    } catch (error) {
      console.log(`  ⚠ Could not fetch tokens: ${(error as Error).message}`);
    }

    return allChecks;
  }

  private async checkDaoPage(): Promise<PageConsistencyCheck[]> {
    console.log(`🏦 Checking DAO page...`);

    try {
      const checks = await this.checker.checkDaoPage();
      const inconsistent = checks.filter((c) => !c.isConsistent);
      console.log(`  ✓ DAO page checked, Inconsistencies: ${inconsistent.length}`);
      return inconsistent;
    } catch (error) {
      console.log(`  ⚠ Could not check DAO page: ${(error as Error).message}`);
      return [];
    }
  }
}
