import { describe, expect, it } from 'vitest';
import { render, screen } from '@/__tests__/utils/test-utils';
import {
  cellLockFacts,
  cellLockSentence,
  cellOwnerLock,
  LockKind,
  lockWords,
  LockWordsView,
  namedLockWords,
  rowLock,
} from '@/components/ui/lock-kind';
import { lockPhrase, partyLockSentence, partyLockWordsText } from '@/lib/lock-words';
import { rowTagClass } from '@/components/ui/row-tag';
import { ckbAddress, JOYID_CODE_HASH } from '@/__tests__/utils/ckb-address';
import { DEFAULT_LOCK_CODE_HASH } from '@/lib/lock-script';
import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';

/** A lock the registry does not name (made up). */
const CODE = `0x${'9b'.repeat(32)}`;
/** JoyID's lock, a real other lock (B4 fix round: fixtures say what they are). */
const JOYID = JOYID_CODE_HASH;

describe('LockKind (R-2)', () => {
  it('shows the registry name with the code hash in the title', () => {
    render(<LockKind address={null} size="row" lockKind="JoyID" lockCodeHash={JOYID} />);
    expect(screen.getByText('JoyID')).toHaveAttribute('title', JOYID);
  });

  // U-09 (re-pinned): the code hash at the size the caller names, the one ellipsis
  it('shows the code hash when the registry names none', () => {
    render(<LockKind address={null} size="row" lockKind={null} lockCodeHash={CODE} />);
    expect(screen.getByText('0x9b9b9b9b…9b9b9b')).toBeInTheDocument();
  });

  it('shows the code hash at the tight size where the caller names it', () => {
    render(<LockKind address={null} size="tight" lockKind={null} lockCodeHash={CODE} />);
    expect(screen.getByText('0x9b9b…9b9b')).toBeInTheDocument();
  });

  // U-15 (UI review 2026-11-14): "Lock kind unavailable" told the reader an internal state. A lock
  // the registry does not name is named by what is known: the word "lock" and its code hash,
  // shortened, linking to the script page of that code hash; one unit that never breaks inside.
  it('names an unregistered lock "lock <code hash>", the hash a link to its script page', () => {
    const { container } = render(
      <LockKind address={null} size="row" lockKind={null} lockCodeHash={CODE} />
    );
    const kind = container.querySelector('[data-lock-kind]') as HTMLElement;
    expect(kind).toHaveTextContent(/^lock 0x9b9b9b9b…9b9b9b$/);
    const link = screen.getByRole('link', { name: '0x9b9b9b9b…9b9b9b' });
    // B4 second fix round (re-pinned on purpose): a lock's script page, as a lock; a bare code hash
    // gives no hash type to carry
    expect(link).toHaveAttribute(
      'href',
      expect.stringMatching(new RegExp(`/script/${CODE}\\?kind=lock$`))
    );
    expect(link).toHaveAttribute('title', CODE);
    expect(kind).toHaveClass('whitespace-nowrap');
  });

  // A4 Chrome sweep (re-pinned): a registered name is words ("Nervape Shadow Lock"), not a unit, so
  // LockKind makes it nowrap in no form; only the unregistered "lock <code hash>" (a label with its
  // id) is. A row chip is a box whose own class keeps its words on one line (rowTagClass).
  it('lets a registered name break between its words (A4 Chrome sweep)', () => {
    render(
      <LockKind address={null} size="row" lockKind="Nervape Shadow Lock" lockCodeHash={JOYID} />
    );
    expect(screen.getByText('Nervape Shadow Lock')).not.toHaveClass('whitespace-nowrap');
  });

  it('keeps its dim form by default and takes a row chip’s classes (table-rows L2-4)', () => {
    const { rerender } = render(
      <LockKind address={null} size="row" lockKind="JoyID" lockCodeHash={JOYID} />
    );
    expect(screen.getByText('JoyID')).toHaveClass('text-text-dim');
    rerender(
      <LockKind
        address={null}
        size="row"
        lockKind="JoyID"
        lockCodeHash={JOYID}
        className={rowTagClass()}
      />
    );
    expect(screen.getByText('JoyID')).toHaveClass('text-meta', 'border', 'leading-4');
    expect(screen.getByText('JoyID')).toHaveAttribute('title', JOYID);
  });

  it('shows nothing for a lock that is not indexed', () => {
    const { container } = render(
      <LockKind address={null} size="row" lockKind={null} lockCodeHash={null} />
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('refuses a kind without its code hash (the contract never sends one)', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() =>
      render(<LockKind size="row" lockKind="JoyID" lockCodeHash={null} address={null} />)
    ).toThrow('lock kind JoyID without its lock code hash');
    log.mockRestore();
  });

  // B4, the lead's ruling: silent for the default lock beside a party, named in a "Lock" column.
  // B4 fix round (re-pinned on purpose): the default lock is the pair (code hash, type), read off
  // the row's address; a bare code hash cannot tell it from another lock on that code hash.
  it('names the default lock only in a column, in quiet words without the column’s tag', () => {
    const address =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
    const tag = rowTagClass('default', 'min-w-0 truncate');
    const lock = { lockKind: 'Default Lock', lockCodeHash: DEFAULT_LOCK_CODE_HASH, address };
    const { container, rerender } = render(<LockKind size="row" {...lock} className={tag} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<LockKind size="row" {...lock} className={tag} place="column" />);
    const plain = screen.getByText('Default Lock');
    expect(plain).toHaveClass('text-text-dim', 'text-meta', 'truncate');
    expect(plain).not.toHaveClass('border');
    rerender(
      <LockKind
        size="row"
        lockKind="JoyID"
        lockCodeHash={JOYID}
        address={null}
        className={tag}
        place="column"
      />
    );
    expect(screen.getByText('JoyID')).toHaveClass('border');
    // the code hash alone (no address): it cannot be told from a lock on it by data, so it is named
    rerender(<LockKind size="row" {...lock} address={null} className={tag} />);
    expect(screen.getByText('Default Lock')).toHaveClass('border');
  });

  it('refuses a row whose address spells another code hash than the row says', () => {
    const log = vi.spyOn(console, 'error').mockImplementation(() => {});
    const address =
      'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqdnnw7qkdnnclfkg59uzn8umtfd2kwxceqxwquc4';
    expect(() =>
      render(<LockKind size="row" lockKind="JoyID" lockCodeHash={JOYID} address={address} />)
    ).toThrow(/spells the lock code hash 0x9bd7e06f.*the row says 0xd00c84f0/);
    log.mockRestore();
  });
});

// Lane B4 third round (review-rules-fixes2, the coordinator's item 5): the tile's refusal clause by
// clause. A cell's address must spell its lock whole: the code hash, the hash type and the args;
// each clause refuses on its own, so deleting any one of them goes red here.
describe('cellLockFacts: an address that spells another lock is refused, clause by clause', () => {
  const args = `0x${'ab'.repeat(20)}`;
  const lock = { codeHash: DEFAULT_LOCK_CODE_HASH, hashType: 'type', args };
  const where = 'Transaction 0x77 output 0';
  const ok = { lock, name: 'Default Lock', address: ckbAddress({ codeHash: lock.codeHash, args }) };

  it('takes an address that spells the cell’s lock whole', () => {
    expect(cellLockFacts(ok, where)).toEqual({
      name: 'Default Lock',
      codeHash: DEFAULT_LOCK_CODE_HASH,
      hashType: 'type',
    });
  });

  it('refuses the same code hash and hash type with other args', () => {
    const other = `0x${'cd'.repeat(20)}`;
    const address = ckbAddress({ codeHash: lock.codeHash, args: other });
    expect(() => cellLockFacts({ ...ok, address }, where)).toThrow(
      `${where}: address ${address} spells the lock ${DEFAULT_LOCK_CODE_HASH} by type, args ${other}; the cell's lock is ${DEFAULT_LOCK_CODE_HASH} by type, args ${args}`
    );
  });

  it('refuses the same code hash and args by another hash type', () => {
    const address = ckbAddress({ codeHash: lock.codeHash, hashType: 'data1', args });
    expect(() => cellLockFacts({ ...ok, address }, where)).toThrow(
      `${where}: address ${address} spells the lock ${DEFAULT_LOCK_CODE_HASH} by data1, args ${args}; the cell's lock is ${DEFAULT_LOCK_CODE_HASH} by type, args ${args}`
    );
  });

  it('refuses another code hash with the same hash type and args', () => {
    const address = ckbAddress({ codeHash: JOYID, args });
    expect(() => cellLockFacts({ ...ok, address }, where)).toThrow(
      `${where}: address ${address} spells the lock ${JOYID} by type, args ${args}; the cell's lock is ${DEFAULT_LOCK_CODE_HASH} by type, args ${args}`
    );
  });

  it('refuses a cell owner whose address spells another lock, named by its cell', () => {
    const address = ckbAddress({ codeHash: lock.codeHash, hashType: 'data', args });
    expect(() =>
      cellOwnerLock({
        lock,
        lockName: 'Default Lock',
        ownerAddress: address,
        txHash: '0x88',
        outputIndex: 3,
      })
    ).toThrow(
      `cell 0x88:3: address ${address} spells the lock ${DEFAULT_LOCK_CODE_HASH} by data, args ${args}; the cell's lock is ${DEFAULT_LOCK_CODE_HASH} by type, args ${args}`
    );
  });
});

// Lane B4 third round (review-rules-fixes2 NOTE 5, the coordinator's item 7): a row that sends its
// lock's hash type (a DAO depositor's lockHashType) hands it to the decision. Where the row has no
// address the decision reads the hash type the row sends; where it has one, the address and the
// row must agree.
describe('rowLock: a row’s own hash type', () => {
  // the row as a depositor sends it; typed here so the test stands before the decision takes it
  const decide = rowLock as unknown as (
    row: {
      lockKind: string | null;
      lockCodeHash: string | null;
      lockHashType: string | null;
      address: string | null;
    },
    place: 'column' | 'inline'
  ) => ReturnType<typeof rowLock>;
  const args = `0x${'ab'.repeat(20)}`;

  it('reads the hash type the row sends where it has no address', () => {
    expect(
      decide(
        {
          lockKind: 'SECP256K1/blake160',
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockHashType: 'data',
          address: null,
        },
        'column'
      )
    ).toEqual({
      form: 'lock',
      name: null,
      codeHash: DEFAULT_LOCK_CODE_HASH,
      hashType: 'data',
      byHashType: 'data',
    });
  });

  it('refuses a row whose address spells another hash type than it sends', () => {
    const address = ckbAddress({ codeHash: DEFAULT_LOCK_CODE_HASH, args });
    expect(() =>
      decide(
        {
          lockKind: 'SECP256K1/blake160',
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockHashType: 'data',
          address,
        },
        'column'
      )
    ).toThrow(
      `address ${address} spells the lock ${DEFAULT_LOCK_CODE_HASH} by type, the row says by data`
    );
  });

  it('refuses a hash type the chain does not have', () => {
    expect(() =>
      decide(
        {
          lockKind: null,
          lockCodeHash: DEFAULT_LOCK_CODE_HASH,
          lockHashType: 'data3',
          address: null,
        },
        'column'
      )
    ).toThrow(`lock ${DEFAULT_LOCK_CODE_HASH}: lock hash type "data3" is not a CKB hash type`);
  });
});

// Lane P2's follow-up (item 6): a SENTENCE must name the default lock (silence beside a party says
// "the default lock"; a sentence has no party to be silent beside), so the one decision has a
// sentence form: the column's answer, never null, said as a noun phrase with its article.
describe('the decision’s sentence form', () => {
  const args = `0x${'ab'.repeat(20)}`;
  const cell = (codeHash: string, hashType: 'type' | 'data', lockName: string | null) => ({
    lock: { codeHash, hashType, args },
    lockName,
    ownerAddress: ckbAddress({ codeHash, hashType, args }),
    txHash: `0x${'77'.repeat(32)}`,
    outputIndex: 3,
  });

  it('names the default lock by its name, where beside a party it is silent', () => {
    const facts = {
      name: 'Default Lock',
      codeHash: DEFAULT_LOCK_CODE_HASH,
      hashType: 'type' as const,
    };
    expect(lockWords(facts, 'inline')).toBeNull();
    expect(lockPhrase(namedLockWords(facts, 'sentence'), 'row')).toBe('a Default Lock');
    expect(cellLockSentence(cell(DEFAULT_LOCK_CODE_HASH, 'type', 'Default Lock'), 'row')).toBe(
      'a Default Lock'
    );
  });

  it('says another lock with its article, a name without "lock" with the word', () => {
    expect(cellLockSentence(cell(JOYID, 'type', 'JoyID'), 'row')).toBe('a JoyID lock');
    expect(
      lockPhrase(
        namedLockWords({ name: 'Omnilock', codeHash: CODE, hashType: 'type' }, 'sentence'),
        'row'
      )
    ).toBe('an Omnilock');
  });

  it('writes a lock the registry does not name by its code hash, a by-data lock with its hash type', () => {
    expect(cellLockSentence(cell(CODE, 'type', null), 'row')).toBe('a lock 0x9b9b9b9b…9b9b9b');
    // the registry's name is the default lock's, not this lock's (B4 fix round)
    expect(cellLockSentence(cell(DEFAULT_LOCK_CODE_HASH, 'data', 'Default Lock'), 'row')).toBe(
      'a lock 0x9bd7e06f…a3cce8 by data'
    );
  });

  it('says a lock that is not indexed in words, never silent', () => {
    expect(
      lockPhrase(namedLockWords({ name: null, codeHash: null, hashType: null }, 'sentence'), 'row')
    ).toBe('a lock not indexed');
  });

  it('refuses a cell whose address spells another lock, naming the cell', () => {
    const wrong = {
      ...cell(JOYID, 'type', 'JoyID'),
      ownerAddress: cell(CODE, 'type', null).ownerAddress,
    };
    expect(() => cellLockSentence(wrong, 'row')).toThrow(`cell 0x${'77'.repeat(32)}:3`);
  });

  it('draws the default lock in a sentence at the line’s size and ink, not as a column’s cell', () => {
    const facts = {
      name: 'Default Lock',
      codeHash: DEFAULT_LOCK_CODE_HASH,
      hashType: 'type' as const,
    };
    render(<LockWordsView words={namedLockWords(facts, 'sentence')} size="row" />);
    const words = screen.getByText('Default Lock');
    expect(words).toHaveAttribute('title', DEFAULT_LOCK_CODE_HASH);
    expect(words).not.toHaveClass('truncate');
    expect(words).not.toHaveClass('text-meta');
  });
});

// Lane P2's third round (item 14; review-p-logic-pages NOTE 1): a named lock's article follows the
// name's first SOUND, not its first letter ("an RGB++ lock", "an M-NFT lock", "an xUDT lock", "a
// JoyID lock", "a UniPass lock"): one small rule in the decision, read here over every name the
// registry holds (docs/metadata/scripts/*.toml and protocols.toml). The table is the reading of
// each name aloud; a name the registry gains fails until it is read here.
describe('the article follows the name’s first sound', () => {
  const METADATA = join(__dirname, '..', '..', '..', 'docs', 'metadata');
  const NAME = /^name\s*=\s*"([^"]+)"/gm;
  const namesIn = (file: string) => [...readFileSync(file, 'utf8').matchAll(NAME)].map((m) => m[1]);
  const registry = new Set([
    ...readdirSync(join(METADATA, 'scripts'))
      .filter((f) => f.endsWith('.toml'))
      .flatMap((f) => namesIn(join(METADATA, 'scripts', f))),
    ...namesIn(join(METADATA, 'protocols.toml')),
  ]);
  /** Each name read aloud: "an" before a vowel sound ("ar-G-B", "em", "ex", "eye", "oh"). */
  const AN = new Set([
    'AlwaysSuccess',
    'Anyone-Can-Pay Lock',
    'EasyToDiscoverType',
    'iCKB Logic',
    'InputTypeProxyLock',
    'M-NFT',
    'M-NFT Class',
    'M-NFT Issuer',
    'OMNI Lock',
    'OutputTypeProxyLock',
    'RGB++',
    'xUDT',
  ]);
  /** "a" before a consonant sound: ".bit" ("dot"), "UDT" and "UniPass" ("you"), "wCKB" ("double-u"). */
  const A = new Set([
    '.bit',
    '.bit Account',
    '.bit Cell',
    '.bit Income Cell',
    '.bit Lock',
    '.bit Reverse Record',
    '.bit Time Index State',
    '.bit Time Info',
    '.cell',
    'Bitcoin SPV Type Lock',
    'Btc Time Lock',
    'ccBTC Asset',
    'ccBTC Oracle Cell',
    'Cells Account',
    'Cells Account Lock',
    'Cells Price',
    'Cells Sale Lock',
    'CHEQUE',
    'CKBFS',
    'COTA',
    'COTA Registry',
    'Default Lock',
    'Default Multisig',
    'did:ckb',
    'Fiber',
    'Fiber Commitment Lock',
    'Fiber Funding Lock',
    'FlashSigner',
    'Force Bridge',
    'Godwoken Challenge Lock',
    'Godwoken Custodian Lock',
    'Godwoken Deposit Lock',
    'Godwoken ETH Account Lock',
    'Godwoken Stake Lock',
    'Godwoken State Validator',
    'Godwoken Withdrawal Lock',
    'JoyID',
    'LockProxyLock',
    'Nervape Shadow Lock',
    'Nervos DAO',
    'NexisDAO dCKB Lock',
    'Nostr',
    'PW Lock',
    'Quantum-Resistant Lock',
    'Simple UDT',
    'Single Use Lock',
    'Spore',
    'Spore Cluster',
    'Spore Cluster Agent',
    'Spore Cluster Proxy',
    'Stable++',
    'Stable++ Asset',
    'Stable++ Intent Lock',
    'Stable++ Pool',
    'Stable++ Vault Lock',
    'TimeLock',
    'Type ID',
    'TypeBurnLock',
    'UDT Limit Order',
    'UniPass',
    'Unique Cell',
    'USDI Asset',
    'UTXOSwap',
    'UTXOSwap Intent Lock',
    'UTXOSwap Proxy Lock',
    'wCKB Asset',
    'WR Owned-Owner',
    'Zero Lock',
  ]);
  const phrase = (name: string) =>
    lockPhrase(namedLockWords({ name, codeHash: CODE, hashType: 'type' }, 'sentence'), 'row');

  it('reads every name the registry holds, and only those', () => {
    // 75 scripts and 7 protocols, two names in both (Nervos DAO, RGB++)
    expect(registry.size).toBe(80);
    expect([...registry].filter((n) => !AN.has(n) && !A.has(n))).toEqual([]);
    expect([...AN, ...A].filter((n) => !registry.has(n))).toEqual([]);
  });

  it.each([...AN].map((n) => [n]))('says "an %s"', (name) => {
    expect(phrase(name)).toMatch(new RegExp(`^an ${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`));
  });

  it.each([...A].map((n) => [n]))('says "a %s"', (name) => {
    expect(phrase(name)).toMatch(new RegExp(`^a ${name.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')}`));
  });

  it('writes the four the review named', () => {
    expect(phrase('RGB++')).toBe('an RGB++ lock');
    expect(phrase('Omnilock')).toBe('an Omnilock');
    expect(phrase('JoyID')).toBe('a JoyID lock');
    expect(phrase('xUDT')).toBe('an xUDT lock');
  });
});

// Lane P2's fifth round (item 3; review-fix-logic-routes S5): on an API one version older a .cell
// party sends `scriptName`, not `lockScriptName`, and the party's lock read "an undefined lock". The
// party's one reader refuses a party without the field by name; it never writes a word that is
// not on chain.
describe('a named party from an API older than this page', () => {
  const older = {
    address: ckbAddress({ codeHash: JOYID, hashType: 'type', args: `0x${'0a'.repeat(22)}` }),
    scriptName: 'JoyID',
  } as unknown as { lockScriptName: string | null; address: string | null };
  it('is refused by name, never written "undefined"', () => {
    expect(() => partyLockWordsText(older, 'row')).toThrow(
      /a \.cell party: the API sends no lockScriptName: it is older than this page/
    );
    expect(() => partyLockSentence(older, 'row')).toThrow(/it is older than this page/);
  });
});
