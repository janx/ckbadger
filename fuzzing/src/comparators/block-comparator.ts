import { config, criticalFields } from '../config';
import type { ComparisonResult, CkbadgerBlock, OfficialBlockResponse, Severity } from '../types';

export class BlockComparator {
  compare(ckbadger: CkbadgerBlock, official: OfficialBlockResponse): ComparisonResult[] {
    const issues: ComparisonResult[] = [];
    const blockId = String(ckbadger.number);
    const attrs = official.data.attributes;

    this.checkCriticalFields(ckbadger, attrs, blockId, issues);
    this.checkTimestamp(ckbadger, attrs, blockId, issues);
    this.checkMinerInfo(ckbadger, attrs, blockId, issues);

    return issues;
  }

  private checkCriticalFields(
    ckbadger: CkbadgerBlock,
    official: OfficialBlockResponse['data']['attributes'],
    blockId: string,
    issues: ComparisonResult[]
  ): void {
    const checks: [string, unknown, unknown][] = [
      ['hash', ckbadger.hash, official.block_hash],
      ['number', ckbadger.number, official.number],
      ['transactionsCount', ckbadger.transactionsCount, official.transactions_count],
      ['proposalsCount', ckbadger.proposalsCount, official.proposals_count],
      ['unclesCount', ckbadger.unclesCount, official.uncles_count],
    ];

    for (const [field, ours, theirs] of checks) {
      if (String(ours) !== String(theirs)) {
        const severity: Severity = criticalFields.block.includes(field) ? 'critical' : 'warning';
        issues.push({
          entity: 'block',
          id: blockId,
          field,
          ckbadger: ours,
          official: theirs,
          severity,
          message: `Block #${blockId}: ${field} mismatch (ckbadger: ${ours}, official: ${theirs})`,
        });
      }
    }
  }

  private checkTimestamp(
    ckbadger: CkbadgerBlock,
    official: OfficialBlockResponse['data']['attributes'],
    blockId: string,
    issues: ComparisonResult[]
  ): void {
    const ourTimestamp = new Date(ckbadger.timestamp).getTime();
    const theirTimestamp = official.timestamp;
    const diff = Math.abs(ourTimestamp - theirTimestamp);

    if (diff > config.tolerance.timestampDiffMs) {
      issues.push({
        entity: 'block',
        id: blockId,
        field: 'timestamp',
        ckbadger: ourTimestamp,
        official: theirTimestamp,
        severity: 'warning',
        message: `Block #${blockId}: timestamp diff ${diff}ms exceeds tolerance`,
      });
    }
  }

  private checkMinerInfo(
    ckbadger: CkbadgerBlock,
    official: OfficialBlockResponse['data']['attributes'],
    blockId: string,
    issues: ComparisonResult[]
  ): void {
    if (ckbadger.minerAddress && official.miner_hash) {
      const ourMiner = ckbadger.minerAddress.toLowerCase();
      const theirMiner = official.miner_hash.toLowerCase();

      if (ourMiner !== theirMiner) {
        issues.push({
          entity: 'block',
          id: blockId,
          field: 'minerAddress',
          ckbadger: ckbadger.minerAddress,
          official: official.miner_hash,
          severity: 'warning',
          message: `Block #${blockId}: miner address mismatch`,
        });
      }
    }
  }
}
