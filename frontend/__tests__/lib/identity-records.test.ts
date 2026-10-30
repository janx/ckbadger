import { describeIdentityRecords } from '@/lib/identity-records';

const rec = (key: string, valueUtf8: string | null, valueHex = '0x00', ttl = 300) => ({
  key,
  valueUtf8,
  valueHex,
  ttl,
  address: null as string | null,
});

describe('describeIdentityRecords (ID-DC1 / ID-DC5)', () => {
  it('names address records by coin and profile records by field, never by the raw key alone', () => {
    const view = describeIdentityRecords([
      { ...rec('address.309', 'ckb1qzda0'), address: 'ckb1qzda0' },
      rec('address.0', 'bc1qxyz'),
      rec('address.60', '0xabc'),
      rec('address.999999', 'zzz'),
      rec('profile.name', 'Albert '),
      rec('profile.location', 'New York, USA'),
      rec('profile.accent', '#476de1'),
      rec('profile.avatar', null, '0x' + '52'.repeat(20184)),
      rec('profile.cover', ''),
      rec('custom.note', 'hi'),
    ]);
    expect(view.addresses.map((a) => [a.coin, a.key, a.coinType])).toEqual([
      ['Bitcoin', 'address.0', '0'],
      ['Ethereum', 'address.60', '60'],
      ['CKB', 'address.309', '309'],
      ['Coin type 999999', 'address.999999', '999999'],
    ]);
    expect(view.addresses[2].ckbAddress).toBe('ckb1qzda0');
    expect(view.profile.map((p) => [p.label, p.value])).toEqual([
      ['Name', { kind: 'text', text: 'Albert' }],
      ['Location', { kind: 'text', text: 'New York, USA' }],
      ['Accent', { kind: 'colour', text: '#476de1' }],
      ['Avatar', { kind: 'bytes', bytes: 20184 }],
      ['Cover', { kind: 'empty' }],
    ]);
    expect(view.other.map((o) => o.key)).toEqual(['custom.note']);
    expect(view.ttls).toEqual([300]);
  });

  it('keeps a colour-looking value that is not a colour as text, never as an inline style', () => {
    const view = describeIdentityRecords([rec('profile.accent', 'red; background: url(x)')]);
    expect(view.profile[0].value).toEqual({ kind: 'text', text: 'red; background: url(x)' });
  });

  it('rejects a record value hex without its 0x prefix', () => {
    expect(() => describeIdentityRecords([rec('profile.avatar', null, 'ffff')])).toThrow(
      /profile.avatar.*0x/
    );
  });
});

it('names an owner-written empty key as empty, never an invented "record" (#17, ID-DC1)', () => {
  const view = describeIdentityRecords([rec('', 'x'), rec('profile.', 'y')]);
  expect(view.other.map((o) => o.key)).toEqual(['']);
  expect(view.profile.map((p) => p.label)).toEqual(['(empty key)']);
});

it('lays the profile out as people read it: name, place, bio, links, then colour, then binary', () => {
  const view = describeIdentityRecords([
    rec('profile.accent', '#476de1'),
    rec('profile.location', 'New York'),
    rec('profile.avatar', null, '0x5249'),
    rec('profile.name', 'Albert'),
    rec('profile.cover', null, '0x5249'),
    rec('profile.bio', 'hi'),
    rec('profile.instagram', 'apl'),
    rec('profile.twitter', 'apl'),
    rec('profile.zodiac', 'leo'),
  ]);
  expect(view.profile.map((p) => p.label)).toEqual([
    'Name',
    'Location',
    'Bio',
    'Twitter',
    'Instagram',
    'Zodiac',
    'Accent',
    'Avatar',
    'Cover',
  ]);
});
