import { render, screen } from '@/__tests__/utils/test-utils';
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

function Facts(props: Parameters<typeof identityFacts>[0]) {
  return (
    <dl>
      {identityFacts(props).map((f) => (
        <div key={f.label} data-fact={f.label}>
          <dt>{f.label}</dt>
          <dd>{f.value}</dd>
        </div>
      ))}
    </dl>
  );
}
const fact = (label: string) => document.querySelector(`[data-fact="${label}"]`);

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
    expect(
      Array.from(document.querySelectorAll('[data-fact]')).map((f) => f.getAttribute('data-fact'))
    ).toEqual(['Owner', 'Expires', 'Registered', 'Records', 'CKB held in its cell']);
    expect(fact('Owner')).toHaveTextContent('.bit Lock · owner and manager are the same key');
    expect(fact('Expires')).toHaveTextContent('2027-02-19');
    expect(fact('Expires')).toHaveTextContent('in 116 days · renewed 2025-02-18');
    expect(fact('Registered')).toHaveTextContent('2024-02-20 08:56:57 UTC+8');
    expect(fact('Registered')).toHaveTextContent('#12,260,151');
    expect(fact('Records')).toHaveTextContent('none set');
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
    expect(fact('Expired')).toHaveTextContent('184 days ago · never renewed');
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
    expect(fact('Sub-names · for sale')).toHaveTextContent('none · no');
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
      'Registered on 2024-02-20 and renewed twice, it is held until 2027-02-19, 116 days from now. Its owner has never changed. No records are set.'
    );
  });

  it('tells a did:ckb it has no expiry and names its server', () => {
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
    expect(screen.getByTestId('identity-summary')).toHaveTextContent(
      'A Web5 identity, created on 2023-11-15. It names a personal data server at pds.example. It has no expiry date.'
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
    expect(
      Array.from(document.querySelectorAll('[data-fact]')).map((f) => f.getAttribute('data-fact'))
    ).toEqual([
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
      "The name's term ended on 2026-04-24, 184 days ago."
    );
  });
});

describe('.cell rail parties (visual pass)', () => {
  it('shows each named party as its address and lock, without an inner label', () => {
    const item = dotcellItem();
    item.owner = {
      hashPrefix: '0x69',
      lockHash: '0x' + '69'.repeat(32),
      address: 'ckb1qqcz6d',
      scriptName: 'Quantum-Resistant Lock',
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
      '/mainnet/address/ckb1qqcz6d'
    );
    expect(fact('Owner')).toHaveTextContent('Quantum-Resistant Lock');
    expect(fact('Owner')).not.toHaveTextContent("named in the name's data");
    expect(fact('Manager')).not.toHaveTextContent('edits its records');
    expect(fact('Manager')).toHaveTextContent('prefix · no uniquely resolved lock');
  });
});
