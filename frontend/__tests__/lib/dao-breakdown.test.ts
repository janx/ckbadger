import { describe, expect, it } from 'vitest';
import { daoStatistics } from '@/__tests__/msw/fixtures/home';
import { compensationParts, secondaryIssuanceParts } from '@/lib/dao-breakdown';

describe('DAO breakdowns', () => {
  it('reads every part exactly and shares them exactly', () => {
    const parts = secondaryIssuanceParts(daoStatistics);
    expect(parts.map((p) => [p.key, p.shannons, p.percent])).toEqual([
      ['mining', BigInt('96304962619313853'), '10.3'],
      ['deposit', BigInt('159838862391653367'), '17.2'],
      ['burnt', BigInt('674731136445379763'), '72.5'],
    ]);
    expect(compensationParts(daoStatistics).map((p) => p.percent)).toEqual(['47.6', '52.4']);
  });

  it('has no parts before anything is issued', () => {
    expect(
      compensationParts({
        ...daoStatistics,
        totalCompensationPaid: '0',
        unclaimedCompensation: '0',
      })
    ).toEqual([]);
  });

  it('fails fast on a missing amount instead of counting it as 0', () => {
    expect(() =>
      secondaryIssuanceParts({ ...daoStatistics, burnt: undefined as unknown as string })
    ).toThrow('burnt "undefined" is not an integer shannon amount');
  });
});
