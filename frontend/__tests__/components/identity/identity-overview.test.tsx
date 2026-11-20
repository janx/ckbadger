import { ckbAddress, namedLockCodeHash, seededAddress } from '@/__tests__/utils/ckb-address';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { AssetMetrics } from '@/components/asset/detail-shell';
import { render, screen, within } from '@/__tests__/utils/test-utils';
import {
  identityFacts,
  IdentitySummary,
  type IdentityProtocol,
} from '@/components/identity/identity-overview';
import { identityTerm } from '@/lib/identity-term';
import type { ItemProtocol } from '@/lib/api';
import {
  bitCellProtocolProducer,
  didProtocolProducer,
  dotbitV4ProtocolProducer,
  dotcellProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';
import { dotcellItem } from '@/__tests__/utils/item-read-fixtures';
import { inkOf, QUIET, READING } from '@/__tests__/utils/ink-role';

const NOW = Date.parse('2026-10-25T13:54:13Z');
const sec = (iso: string) => String(Date.parse(iso) / 1000);

function valar(): Extract<ItemProtocol, { kind: 'dotbit' }> {
  const p = structuredClone(dotbitV4ProtocolProducer) as Extract<ItemProtocol, { kind: 'dotbit' }>;
  const v0 = p.versions[0];
  v0.decoded.registeredAt = sec('2024-02-20T00:56:57Z');
  v0.decoded.expiredAt = sec('2025-02-20T00:56:57Z');
  v0.decoded.owner = { algorithm: 8, argsHex: '0x07ef' };
  v0.decoded.manager = { algorithm: 8, argsHex: '0x07ef' };
  v0.decoded.records = [];
  v0.cell.timestamp = String(Date.parse('2024-02-20T00:56:57Z'));
  v0.cell.lockName = '.bit Lock';
  const renew = (when: string, exp: string) => {
    const v = structuredClone(v0);
    v.cell.timestamp = String(Date.parse(when));
    v.cell.txHash = '0x' + String(Date.parse(when)).padStart(64, '0');
    v.decoded.expiredAt = sec(exp);
    return v;
  };
  p.versions = [
    v0,
    renew('2025-02-04T00:00:00Z', '2026-02-20T00:56:57Z'),
    renew('2025-02-18T00:00:00Z', '2027-02-19T00:56:57Z'),
  ];
  return p;
}

// Consistency round C7: the facts are the metrics row; rendered through it, each fact is the
// row's item (its value, then its inline note).
function Facts(props: Parameters<typeof identityFacts>[0]) {
  return <AssetMetrics items={identityFacts(props)} />;
}
const facts = () => Array.from(document.querySelectorAll('[data-role="metrics"] dl > div'));
const fact = (label: string) =>
  facts().find((f) => f.querySelector('dt')?.textContent === label) ?? null;
const factLabels = () => facts().map((f) => f.querySelector('dt')!.textContent);

// Lane p-assets (the lock-words gate's known writer 5): the Owner fact's lock is the one
// decision's (components/ui/lock-kind.tsx): nothing beside the address for the default lock, where
// the note wrote the API's name, or "lock not named".
it('names an owner’s lock by the one decision: nothing for the default lock', () => {
  const protocol = valar();
  const last = protocol.versions.at(-1)!.cell;
  const args = `0x${'ab'.repeat(20)}`;
  last.lock = { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'type', args };
  last.lockName = 'SECP256K1/blake160';
  last.ownerAddress = ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, args });
  render(
    <Facts
      protocol={protocol}
      term={identityTerm(protocol)}
      now={NOW}
      isLive
      createdAtBlock={12260151}
    />
  );
  expect(fact('Owner')).toHaveTextContent(/owner and manager are the same key$/);
  expect(fact('Owner')).not.toHaveTextContent('SECP256K1');
  expect(fact('Owner')).not.toHaveTextContent('lock not named');
});

describe('identity facts rail (ID-C6)', () => {
  it('gives .bit its domain facts: owner, expiry with the last renewal, registration, records', () => {
    const protocol = valar();
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        createdAtBlock={12260151}
      />
    );
    expect(factLabels()).toEqual([
      'Owner',
      'Expires',
      'Registered',
      'Records',
      'CKB held in its cell',
    ]);
    expect(fact('Owner')).toHaveTextContent('.bit Lock · owner and manager are the same key');
    expect(fact('Expires')).toHaveTextContent('2027-02-19');
    // Lane P: how far off is the metric's hover; the note keeps the last renewal.
    expect(fact('Expires')).toHaveTextContent('renewed 2025-02-18');
    expect(fact('Expires')).not.toHaveTextContent('in 116 days');
    expect(fact('Expires')).toHaveAttribute('title', 'expires in 116\u00a0days');
    expect(fact('Registered')).toHaveTextContent('2024-02-20');
    expect(fact('Registered')).toHaveAttribute('title', 'registered 2024-02-20 08:56:57 UTC+8');
    expect(fact('Registered')).toHaveTextContent('#12,260,151');
    expect(fact('Records')).toHaveTextContent('none set');
    // BRIEF-roles-rest parts A and E: a metric's note is quiet (R13), its values are not: the
    // block a name was registered at (a link), the day of its last renewal and the owner's keys'
    // state read in the reading ink; "renewed" stays a quiet label (the notes were quiet whole)
    expect(
      inkOf(within(fact('Registered') as HTMLElement).getByRole('link', { name: '#12,260,151' }))
    ).toBe(READING);
    expect(inkOf(within(fact('Expires') as HTMLElement).getByText('2025-02-18'))).toBe(READING);
    expect(inkOf(within(fact('Expires') as HTMLElement).getByText(/^renewed/))).toBe(QUIET);
    expect(
      inkOf(within(fact('Owner') as HTMLElement).getByText('owner and manager are the same key'))
    ).toBe(READING);
    // Raw protocol internals are tier 2, not the rail (ID-B1, ID-DC7).
    expect(document.body).not.toHaveTextContent(/status|witness version|Physical cell owner/);
  });

  it('marks an expired .bit Cell and names its .bit account', () => {
    const protocol = structuredClone(bitCellProtocolProducer) as Extract<
      ItemProtocol,
      { kind: 'bit_cell' }
    >;
    protocol.versions[0].decoded.expiredAt = sec('2026-04-24T04:57:22Z');
    protocol.versions[0].decoded.linkedAccount = {
      id: '0x' + 'c9'.repeat(20),
      name: '888899.bit',
      isLive: false,
      registeredAt: null,
      expiredAt: null,
    };
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        createdAtBlock={100}
      />
    );
    expect(fact('Expired')).toHaveTextContent('2026-04-24');
    // Lane P: the date is the value; how long ago is the metric's hover.
    expect(fact('Expired')).toHaveTextContent(/^Expired2026-04-24 · never renewed$/);
    // BRIEF-roles-rest part E: "never renewed" is the term's state, a value in the reading ink
    // (it stood quiet in the note); "not live" the .bit account's
    expect(inkOf(within(fact('Expired') as HTMLElement).getByText('never renewed'))).toBe(READING);
    expect(inkOf(within(fact('.bit account') as HTMLElement).getByText('not live'))).toBe(READING);
    expect(fact('Expired')).toHaveAttribute('title', 'expired 184\u00a0days ago');
    expect(fact('Minted')).toBeInTheDocument();
    expect(fact('.bit account')).toHaveTextContent('888899.bit · not live');
  });

  it('gives did:ckb no expiry and its creation and last update', () => {
    const protocol = structuredClone(didProtocolProducer) as IdentityProtocol;
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        createdAtBlock={100}
      />
    );
    expect(fact('Expiry')).toHaveTextContent('none');
    expect(fact('Created')).toBeInTheDocument();
    expect(fact('Last updated')).toBeNull();
  });

  it('gives .cell its named owner and manager, grace, records and sub-names', () => {
    const item = dotcellItem();
    const protocol = structuredClone(dotcellProtocolProducer) as IdentityProtocol;
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol, { graceEndsAt: item.graceEndsAt })}
        now={NOW}
        isLive
        createdAtBlock={100}
        dotcell={item}
      />
    );
    expect(fact('Owner')).toHaveTextContent(item.owner.hashPrefix);
    expect(fact('Manager')).toHaveTextContent(item.manager.hashPrefix);
    expect(fact('Expires')).toHaveTextContent('grace to');
    expect(fact('Records')).toHaveTextContent('0');
    // re-pinned on purpose (lane P2's follow-up, a standing cause): the sub-names and the sale are
    // two facts (one metric said "none · no"), and a name not for sale draws no sale metric
    expect(fact('Sub-names')).toHaveTextContent('none');
    expect(screen.queryByText('For sale')).toBeNull();
  });

  // Readability review 2026-11-18 (lane A): on verification.cell at 1280 "CKB held in its cell"
  // broke into five lines, "240" / ".00000000 CKB" / … : the amount was allowed a break before its
  // decimals, and its column (one seventh of the row, 148.6 px) was under the 148.8 px its value
  // needs on one line. A figure is one unit; the row gives each column at least its content.
  it("keeps the .cell's CKB held one unit, its integer never parted from its decimals", () => {
    const item = dotcellItem();
    const protocol = structuredClone(dotcellProtocolProducer) as IdentityProtocol;
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol, { graceEndsAt: item.graceEndsAt })}
        now={NOW}
        isLive
        createdAtBlock={100}
        dotcell={item}
      />
    );
    const value = fact('CKB held in its cell')!.querySelector('dd [data-ckb-amount]')!;
    expect(value).toHaveClass('whitespace-nowrap');
    expect(value).not.toHaveClass('flex-wrap');
  });
});

// Lane P2's follow-up (a standing cause, empty identity metrics): a metric with nothing to say is
// not drawn. A .bit version that stores no expiry drew "Expires · not recorded"; the page's
// sentence says it once ("Version N of its cell records no expiry.").
describe('identity facts draw no metric with nothing to say', () => {
  it('draws no expiry for a version that stores none', () => {
    const protocol = valar();
    for (const v of protocol.versions) v.decoded.expiredAt = null;
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        createdAtBlock={12260151}
      />
    );
    expect(factLabels()).not.toContain('Expires');
    expect(document.body).not.toHaveTextContent('not recorded');
    expect(factLabels()).toContain('Registered');
  });
});

describe('identity summary (ID-C5)', () => {
  it('says how long a .bit is held, that its owner never changed, and its records', () => {
    const protocol = valar();
    render(
      <IdentitySummary
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        subject="valar.bit"
      />
    );
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      'Registered on 2024-02-20 and renewed twice, it is held until 2027-02-19. Its owner has never changed. No records are set.'
    );
    expect(
      within(screen.getByTestId('identity-summary')).getByText('2027-02-19').closest('[title]')
    ).toHaveAttribute('title', '116\u00a0days from now');
  });

  // renamed (R8): it was "tells a did:ckb it has no expiry and names its server"
  it('names a did:ckb’s server; that it has no expiry is the lede’s', () => {
    const protocol = structuredClone(didProtocolProducer) as IdentityProtocol;
    render(
      <IdentitySummary
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        subject="alice.test"
      />
    );
    // Re-pinned on purpose (readability R8, lane p-assets): that it is live with no expiry is the
    // page's lede now (IdentityPositioning), said once; "in words" keeps the rest (it ended "… It
    // has no expiry date.")
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      /^A Web5 identity, created on 2023-11-15\. It names a personal data server at pds\.example\.$/
    );
  });
});

// F2 identities (round 2, visual pass): R2b-b `.bit` witness timing and sub-account fields.
describe('.bit witness facts (ID-B1, ID-C6)', () => {
  function withWitness() {
    const p = valar();
    p.versions.forEach((v, i) => {
      Object.assign(v.decoded, {
        updatedAt: null,
        lastTransferAccountAt: '0',
        lastEditManagerAt: '0',
        lastEditRecordsAt: i === 0 ? '0' : sec('2024-06-04T09:15:02Z'),
        enableSubAccount: i === 0 ? 0 : 1,
        renewSubAccountPrice: '100000000',
      });
    });
    return p;
  }
  it('dates the last record edit and says since when sub-accounts are on', () => {
    const protocol = withWitness();
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        createdAtBlock={1}
      />
    );
    expect(factLabels()).toEqual([
      'Owner',
      'Expires',
      'Registered',
      'Records',
      'Sub-accounts',
      'CKB held in its cell',
    ]);
    expect(fact('Records')).toHaveTextContent('none set');
    expect(fact('Records')).toHaveTextContent('last edited 2024-06-04');
    expect(fact('Sub-accounts')).toHaveTextContent('enabled');
    expect(fact('Sub-accounts')).toHaveTextContent('since 2025-02-04');
  });
  it('adds the sub-account switch to the summary', () => {
    const protocol = withWitness();
    render(
      <IdentitySummary
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        subject="valar.bit"
      />
    );
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      'No records are set; sub-accounts are on.'
    );
  });
});

// Lane P2's follow-up (item 6): the .bit Cell summary says the lock it was minted into and the one
// that holds it now through the decision's sentence form (it said the API's name as "a <name>
// wallet", "a wallet whose lock is not named" when there was none, and its own article rule)
describe('.bit Cell summary names its locks through the decision', () => {
  it('says a lock the registry does not name by its code hash, and the default lock by its name', () => {
    const protocol = structuredClone(bitCellProtocolProducer) as Extract<
      ItemProtocol,
      { kind: 'bit_cell' }
    >;
    const summary = () =>
      render(
        <IdentitySummary
          protocol={protocol}
          term={identityTerm(protocol)}
          now={NOW}
          isLive
          subject="888899.bit"
        />
      );
    const { unmount } = summary();
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      /Minted on \S+ into a lock 0x41414141…414141 that still holds it\./
    );
    expect(screen.getByTestId('identity-summary')).not.toHaveTextContent('wallet');
    unmount();
    const args = `0x${'01'.repeat(20)}`;
    for (const v of protocol.versions)
      Object.assign(v.cell, {
        lock: { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'type', args },
        lockName: 'Default Lock',
        ownerAddress: ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, args }),
      });
    summary();
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      /into a Default Lock that still holds it\./
    );
  });
});

describe('.bit Cell summary when the term has ended', () => {
  it('reads "The name\'s term ended on …"', () => {
    const protocol = structuredClone(bitCellProtocolProducer) as IdentityProtocol;
    (protocol.versions[0].decoded as { expiredAt: string }).expiredAt = sec('2026-04-24T04:57:22Z');
    render(
      <IdentitySummary
        protocol={protocol}
        term={identityTerm(protocol)}
        now={NOW}
        isLive
        subject="888899.bit"
      />
    );
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      "The name's term ended on 2026-04-24."
    );
    // Lane P: how long ago is the date's hover.
    expect(
      within(screen.getByTestId('identity-summary')).getByText('2026-04-24').closest('[title]')
    ).toHaveAttribute('title', '184\u00a0days ago');
  });
});

describe('.cell rail parties (visual pass)', () => {
  it('shows each named party as its address and lock, without an inner label', () => {
    const item = dotcellItem();
    item.owner = {
      hashPrefix: '0x69',
      lockHash: '0x' + '69'.repeat(32),
      // re-pinned on purpose (lane P2's third round, item 12): a party's lock is read off its address by the one decision, so the fixture's address is a real one (it was a made-up string)
      address: seededAddress('qqcz6d', namedLockCodeHash('Quantum-Resistant Lock')),
      lockScriptName: 'Quantum-Resistant Lock',
    };
    const protocol = structuredClone(dotcellProtocolProducer) as IdentityProtocol;
    render(
      <Facts
        protocol={protocol}
        term={identityTerm(protocol, { graceEndsAt: item.graceEndsAt })}
        now={NOW}
        isLive
        createdAtBlock={100}
        dotcell={item}
      />
    );
    expect(fact('Owner')!.querySelector('a')).toHaveAttribute(
      'href',
      `/mainnet/address/${item.owner.address}`
    );
    expect(fact('Owner')).toHaveTextContent('Quantum-Resistant Lock');
    expect(fact('Owner')).not.toHaveTextContent("named in the name's data");
    expect(fact('Manager')).not.toHaveTextContent('edits its records');
    // re-pinned on purpose (lane P2's fifth round, NOTE 7): one word for a prefix no lock answers
    // uniquely, the fact's and the summary's (it was "prefix · no uniquely resolved lock" here and
    // "a lock not indexed owns it" in the summary)
    expect(fact('Manager')).toHaveTextContent('prefix · lock not uniquely resolved');
  });
});

// Lane P2's third round (item 12c, the test review's MUST-1): a .cell party's lock is said through
// the one decision, off the party's address, which spells the lock: a lock on the default lock's
// code hash by data is not the default lock, whose name the registry sends for that code hash, so
// it reads "lock 0x9bd7e06f…a3cce8 by data", never "Default Lock"; the summary says it in the
// decision's sentence form, the facts in its column words.
describe('.cell parties name their locks through the decision', () => {
  const byData = (seed: string) => {
    const args = '0x' + seed.repeat(20);
    return {
      hashPrefix: '0x' + seed.repeat(20),
      lockHash: '0x' + seed.repeat(32),
      address: ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'data', args }),
      lockScriptName: 'Default Lock',
    };
  };
  it('writes a by-data lock on the default code hash by its code hash, in the facts and the summary', () => {
    const item = dotcellItem();
    const protocol = structuredClone(dotcellProtocolProducer) as Extract<
      IdentityProtocol,
      { kind: 'dotcell' }
    >;
    const first = protocol.versions[0];
    const second = structuredClone(first);
    second.cell.timestamp = String(Number(first.cell.timestamp) + 86_400_000);
    second.cell.txHash = '0x' + '77'.repeat(32);
    second.decoded.owner = byData('6a');
    second.decoded.manager = byData('6b');
    protocol.versions = [first, second];
    const dotcell = { ...item, owner: byData('6a'), manager: byData('6b') };
    render(
      <>
        <Facts
          protocol={protocol}
          term={identityTerm(protocol, { graceEndsAt: dotcell.graceEndsAt })}
          now={NOW}
          isLive
          createdAtBlock={100}
          dotcell={dotcell}
        />
        <IdentitySummary
          protocol={protocol}
          term={identityTerm(protocol, { graceEndsAt: dotcell.graceEndsAt })}
          now={NOW}
          isLive
          subject="alice.cell"
        />
      </>
    );
    expect(fact('Owner')).toHaveTextContent('lock 0x9bd7e06f…a3cce8 by data');
    expect(fact('Manager')).toHaveTextContent('lock 0x9bd7e06f…a3cce8 by data');
    const summary = screen.getByTestId('identity-summary');
    expect(summary).toHaveTextContent(
      /a lock 0x9bd7e06f…a3cce8 by data owns it while a lock 0x9bd7e06f…a3cce8 by data manages its records/
    );
    expect(document.body).not.toHaveTextContent('Default Lock');
  });
});

// Lane P2's fifth round (NOTE 7): a .cell owner whose prefix no lock answers uniquely is said in
// one word on the page: the summary said "a lock not indexed owns it", the decision's answer for a
// lock with no code hash and no name, beside the fact's "no uniquely resolved lock".
it('says an unresolved .cell owner in one word, in the summary and the fact', () => {
  const protocol = structuredClone(dotcellProtocolProducer) as Extract<
    IdentityProtocol,
    { kind: 'dotcell' }
  >;
  const first = protocol.versions[0];
  const second = structuredClone(first);
  second.cell.timestamp = String(Number(first.cell.timestamp) + 86_400_000);
  second.cell.txHash = '0x' + '78'.repeat(32);
  second.decoded.owner = {
    hashPrefix: '0x' + '6c'.repeat(20),
    lockHash: null,
    address: null,
    lockScriptName: null,
  };
  protocol.versions = [first, second];
  const item = dotcellItem();
  const dotcell = { ...item, owner: second.decoded.owner };
  render(
    <>
      <Facts
        protocol={protocol}
        term={identityTerm(protocol, { graceEndsAt: dotcell.graceEndsAt })}
        now={NOW}
        isLive
        createdAtBlock={100}
        dotcell={dotcell}
      />
      <IdentitySummary
        protocol={protocol}
        term={identityTerm(protocol, { graceEndsAt: dotcell.graceEndsAt })}
        now={NOW}
        isLive
        subject="alice.cell"
      />
    </>
  );
  expect(screen.getByTestId('identity-summary')).toHaveTextContent(
    /a lock not uniquely resolved owns it/
  );
  expect(fact('Owner')).toHaveTextContent('prefix · lock not uniquely resolved');
  expect(document.body).not.toHaveTextContent('not indexed');
});
