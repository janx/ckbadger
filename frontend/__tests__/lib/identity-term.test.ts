import { identityTerm, ownerRun, ownerRuns, termDays } from '@/lib/identity-term';
import type { ItemProtocol } from '@/lib/api';
import {
  bitCellProtocolProducer,
  didProtocolProducer,
  dotbitV4ProtocolProducer,
  dotcellProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';

const DAY = 86_400_000;
const sec = (iso: string) => String(Date.parse(iso) / 1000);
const ms = (iso: string) => String(Date.parse(iso));

/** valar.bit's history, shaped like the API: one registration, two renewals, three edits. */
function valar(): Extract<ItemProtocol, { kind: 'dotbit' }> {
  const p = structuredClone(dotbitV4ProtocolProducer) as Extract<ItemProtocol, { kind: 'dotbit' }>;
  const base = p.versions[0];
  const at = (
    when: string,
    expires: string,
    owner = '0x6666666666666666666666666666666666666666'
  ) => {
    const v = structuredClone(base);
    v.cell.timestamp = ms(when);
    v.cell.txHash = '0x' + Math.abs(Date.parse(when)).toString(16).padStart(64, '0');
    v.decoded.registeredAt = sec('2024-02-20T00:56:57Z');
    v.decoded.expiredAt = sec(expires);
    v.decoded.owner = { algorithm: 8, argsHex: owner };
    return v;
  };
  p.versions = [
    at('2024-02-20T00:56:57Z', '2025-02-20T00:56:57Z'),
    at('2024-06-04T00:00:00Z', '2025-02-20T00:56:57Z'),
    at('2025-02-04T00:00:00Z', '2026-02-20T00:56:57Z'),
    at('2025-02-18T00:00:00Z', '2027-02-19T00:56:57Z'),
    at('2025-03-01T00:00:00Z', '2027-02-19T00:56:57Z'),
  ];
  return p;
}

describe('identityTerm (ID-C7)', () => {
  it('reads a .bit term from the witness registration and each version expiry', () => {
    const term = identityTerm(valar());
    if (term.kind !== 'term') throw new Error('expected a term');
    expect(term.periods.map((p) => p.label)).toEqual([
      'paid at registration',
      'renewed 2025-02-04',
      'renewed 2025-02-18',
    ]);
    expect(term.periods[0].start).toBe(Date.parse('2024-02-20T00:56:57Z'));
    expect(term.periods[1].start).toBe(term.periods[0].end);
    expect(term.expiresAt).toBe(Date.parse('2027-02-19T00:56:57Z'));
    expect(term.renewals.map((r) => r.at)).toEqual([
      Date.parse('2025-02-04T00:00:00Z'),
      Date.parse('2025-02-18T00:00:00Z'),
    ]);
    expect(term.graceEnd).toBeUndefined();
  });

  it('draws only the current .bit registration when an earlier one was recycled', () => {
    const p = valar();
    p.versions[0].decoded.registeredAt = sec('2020-01-01T00:00:00Z');
    p.versions[0].decoded.expiredAt = sec('2021-01-01T00:00:00Z');
    const term = identityTerm(p);
    if (term.kind !== 'term') throw new Error('expected a term');
    expect(term.periods[0].label).toBe('paid at registration');
    expect(term.periods[0].start).toBe(Date.parse('2024-02-20T00:56:57Z'));
    expect(term.periods[0].end).toBe(Date.parse('2025-02-20T00:56:57Z'));
  });

  it('fails with the version when an expiry moves backwards inside one registration', () => {
    const p = valar();
    p.versions[3].decoded.expiredAt = sec('2025-01-01T00:00:00Z');
    expect(() => identityTerm(p)).toThrow(/expiry moved back.*version 4/);
  });

  it('reports an unrecorded .bit expiry instead of drawing a term', () => {
    const p = valar();
    p.versions[4].decoded.expiredAt = null;
    expect(identityTerm(p)).toEqual({ kind: 'unrecorded', version: 5 });
  });

  it('starts a .bit Cell term at minting, and a .cell term at registration with its grace', () => {
    const bit = identityTerm(bitCellProtocolProducer as ItemProtocol);
    if (bit.kind !== 'term') throw new Error('expected a term');
    expect(bit.periods).toEqual([
      { label: 'from minting', start: 1700000000100, end: 1900000000 * 1000 },
    ]);
    const cell = identityTerm(dotcellProtocolProducer as ItemProtocol, {
      graceEndsAt: 1900000000 + 30 * 86_400,
    });
    if (cell.kind !== 'term') throw new Error('expected a term');
    expect(cell.periods[0].label).toBe('registered');
    expect(cell.graceEnd).toBe((1900000000 + 30 * 86_400) * 1000);
    expect(cell.graceEnd! - cell.expiresAt).toBe(30 * DAY);
  });

  it('has no term for did:ckb, which never expires', () => {
    expect(identityTerm(didProtocolProducer as ItemProtocol)).toEqual({ kind: 'none' });
  });
});

describe('ownerRun', () => {
  it('finds when the current owner took over and how many owners came before', () => {
    const p = valar();
    expect(ownerRun(p.versions, (v) => v.decoded.owner.argsHex)).toEqual({
      since: p.versions[0],
      changes: 0,
    });
    p.versions[3].decoded.owner = { algorithm: 8, argsHex: '0x' + '77'.repeat(20) };
    p.versions[4].decoded.owner = { algorithm: 8, argsHex: '0x' + '77'.repeat(20) };
    expect(ownerRun(p.versions, (v) => v.decoded.owner.argsHex)).toEqual({
      since: p.versions[3],
      changes: 1,
    });
  });
});

describe('ownerRuns (ID-DC4)', () => {
  it('lists every owner in order with the version where each took over', () => {
    const p = valar();
    const ids = ['a', 'a', 'b', 'c', 'c'];
    expect(
      ownerRuns(p.versions, (v) => ids[p.versions.indexOf(v)]).map((r) => [r.owner, r.since])
    ).toEqual([
      ['a', p.versions[0]],
      ['b', p.versions[2]],
      ['c', p.versions[3]],
    ]);
  });
});

describe('termDays', () => {
  it('counts whole days between two instants, either way round', () => {
    const t = Date.parse('2026-10-25T13:54:13Z');
    expect(termDays(Date.parse('2027-02-19T00:56:57Z'), t)).toBe(116);
    expect(termDays(t, Date.parse('2027-02-19T00:56:57Z'))).toBe(116);
  });
});
