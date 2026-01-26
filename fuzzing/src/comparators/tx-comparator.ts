import { criticalFields } from '../config';
import type {
  ComparisonResult,
  CkbadgerTransactionDetail,
  OfficialTransactionResponse,
  Severity,
} from '../types';

export class TxComparator {
  compare(
    ckbadger: CkbadgerTransactionDetail,
    official: OfficialTransactionResponse
  ): ComparisonResult[] {
    const issues: ComparisonResult[] = [];
    const txId = ckbadger.hash;
    const attrs = official.data.attributes;

    this.checkCriticalFields(ckbadger, attrs, txId, issues);
    this.checkInputsOutputs(ckbadger, attrs, txId, issues);
    this.checkFee(ckbadger, attrs, txId, issues);

    return issues;
  }

  private checkCriticalFields(
    ckbadger: CkbadgerTransactionDetail,
    official: OfficialTransactionResponse['data']['attributes'],
    txId: string,
    issues: ComparisonResult[]
  ): void {
    const checks: [string, unknown, unknown][] = [
      ['hash', ckbadger.hash, official.transaction_hash],
      ['blockNumber', ckbadger.blockNumber, official.block_number],
      ['isCellbase', ckbadger.isCellbase, official.is_cellbase],
    ];

    for (const [field, ours, theirs] of checks) {
      if (String(ours) !== String(theirs)) {
        const severity: Severity = criticalFields.transaction.includes(field)
          ? 'critical'
          : 'warning';
        issues.push({
          entity: 'transaction',
          id: txId,
          field,
          ckbadger: ours,
          official: theirs,
          severity,
          message: `TX ${this.truncateHash(txId)}: ${field} mismatch`,
        });
      }
    }
  }

  private checkInputsOutputs(
    ckbadger: CkbadgerTransactionDetail,
    official: OfficialTransactionResponse['data']['attributes'],
    txId: string,
    issues: ComparisonResult[]
  ): void {
    const ourInputsCount = ckbadger.inputsCount;
    const theirInputsCount = official.display_inputs.length;

    if (ourInputsCount !== theirInputsCount) {
      issues.push({
        entity: 'transaction',
        id: txId,
        field: 'inputsCount',
        ckbadger: ourInputsCount,
        official: theirInputsCount,
        severity: 'critical',
        message: `TX ${this.truncateHash(txId)}: inputs count mismatch (${ourInputsCount} vs ${theirInputsCount})`,
      });
    }

    const ourOutputsCount = ckbadger.outputsCount;
    const theirOutputsCount = official.display_outputs.length;

    if (ourOutputsCount !== theirOutputsCount) {
      issues.push({
        entity: 'transaction',
        id: txId,
        field: 'outputsCount',
        ckbadger: ourOutputsCount,
        official: theirOutputsCount,
        severity: 'critical',
        message: `TX ${this.truncateHash(txId)}: outputs count mismatch (${ourOutputsCount} vs ${theirOutputsCount})`,
      });
    }
  }

  private checkFee(
    ckbadger: CkbadgerTransactionDetail,
    official: OfficialTransactionResponse['data']['attributes'],
    txId: string,
    issues: ComparisonResult[]
  ): void {
    if (ckbadger.isCellbase) return;

    const ourFee = BigInt(ckbadger.fee);
    const theirFee = BigInt(official.transaction_fee);

    if (ourFee !== theirFee) {
      issues.push({
        entity: 'transaction',
        id: txId,
        field: 'fee',
        ckbadger: ckbadger.fee,
        official: official.transaction_fee,
        severity: 'warning',
        message: `TX ${this.truncateHash(txId)}: fee mismatch (${ourFee} vs ${theirFee})`,
      });
    }
  }

  private truncateHash(hash: string): string {
    return `${hash.slice(0, 10)}...${hash.slice(-6)}`;
  }
}
