import { criticalFields } from '../config';
import type {
  ComparisonResult,
  CkbadgerAddress,
  OfficialAddressResponse,
  Severity,
} from '../types';

export class AddressComparator {
  compare(ckbadger: CkbadgerAddress, official: OfficialAddressResponse): ComparisonResult[] {
    const issues: ComparisonResult[] = [];
    const addrId = ckbadger.address ?? ckbadger.lockScriptHash;
    const attrs = official.data.attributes;

    this.checkBalance(ckbadger, attrs, addrId, issues);
    this.checkCellsCount(ckbadger, attrs, addrId, issues);
    this.checkTransactionsCount(ckbadger, attrs, addrId, issues);

    return issues;
  }

  private checkBalance(
    ckbadger: CkbadgerAddress,
    official: OfficialAddressResponse['data']['attributes'],
    addrId: string,
    issues: ComparisonResult[]
  ): void {
    const ourBalance = BigInt(ckbadger.balance);
    const theirBalance = BigInt(official.balance);

    if (ourBalance !== theirBalance) {
      const severity: Severity = criticalFields.address.includes('balance')
        ? 'critical'
        : 'warning';
      issues.push({
        entity: 'address',
        id: addrId,
        field: 'balance',
        ckbadger: ckbadger.balance,
        official: official.balance,
        severity,
        message: `Address ${this.truncateAddr(addrId)}: balance mismatch (diff: ${ourBalance - theirBalance})`,
      });
    }
  }

  private checkCellsCount(
    ckbadger: CkbadgerAddress,
    official: OfficialAddressResponse['data']['attributes'],
    addrId: string,
    issues: ComparisonResult[]
  ): void {
    const ourCount = ckbadger.liveCellsCount;
    const theirCount = parseInt(official.live_cells_count, 10);

    if (ourCount !== theirCount) {
      issues.push({
        entity: 'address',
        id: addrId,
        field: 'liveCellsCount',
        ckbadger: ourCount,
        official: theirCount,
        severity: 'warning',
        message: `Address ${this.truncateAddr(addrId)}: live cells count mismatch (${ourCount} vs ${theirCount})`,
      });
    }
  }

  private checkTransactionsCount(
    ckbadger: CkbadgerAddress,
    official: OfficialAddressResponse['data']['attributes'],
    addrId: string,
    issues: ComparisonResult[]
  ): void {
    const ourCount = ckbadger.transactionsCount;
    const theirCount = parseInt(official.transactions_count, 10);
    const diff = Math.abs(ourCount - theirCount);

    if (diff > 0) {
      const severity: Severity = diff > 10 ? 'warning' : 'info';
      issues.push({
        entity: 'address',
        id: addrId,
        field: 'transactionsCount',
        ckbadger: ourCount,
        official: theirCount,
        severity,
        message: `Address ${this.truncateAddr(addrId)}: transactions count diff ${diff}`,
      });
    }
  }

  private truncateAddr(addr: string): string {
    if (addr.length <= 20) return addr;
    return `${addr.slice(0, 12)}...${addr.slice(-8)}`;
  }
}
