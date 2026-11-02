'use client';
import type { ReactNode } from 'react';
import { ByteScaleMap, type ScaleSegment } from '@/components/ui/hex-map';
import { AppLink } from '@/components/ui/app-link';
import { hexBytes } from '@/lib/hex-bytes';
import { useDetailTier } from '@/components/ui/detail-layout';
import { cn } from '@/lib/utils';

/** The Address board's inks for the three parts of a script. */
export const SCRIPT_INKS = {
  codeHash: '#7aa7d6',
  hashType: '#dee2ec',
  args: '#2edba3',
} as const;

/**
 * RFC 0022 / 0032: the byte a hash type is serialized as, and how the code is found by it.
 */
const HASH_TYPES: Record<string, { byte: string; words: string }> = {
  data: {
    byte: '0x00',
    words: 'the code is found by its data hash and runs on CKB-VM version 0; it cannot change',
  },
  type: { byte: '0x01', words: 'the code is found by its type id, so it can be upgraded' },
  data1: {
    byte: '0x02',
    words: 'the code is found by its data hash and runs on CKB-VM version 1; it cannot change',
  },
  data2: {
    byte: '0x04',
    words: 'the code is found by its data hash and runs on CKB-VM version 2; it cannot change',
  },
};

export interface ScriptByteMapProps {
  /** Which role the script plays; a family whose role is unknown may omit it. */
  kind?: 'lock' | 'type';
  /** The script's three parts; null draws a script family's shape (each cell sets its args). */
  script: { codeHash: string; hashType: string; args: string } | null;
  /** Registry name of the code. */
  name?: string | null;
  /** Where the code hash links (the script page); required with a script. */
  href?: string;
  /** After the code hash: what this code is for here. */
  codeNote?: ReactNode;
  /** After the args: what they are for this script. */
  argsNote?: ReactNode;
}

/**
 * A script as its occupied bytes (consistency C6, Address board tier 2): code_hash 32 · hash_type 1
 * · args N drawn to scale with the names inside and an offsets ruler, then one row per part — the
 * code hash linked to its script page, the hash type with how it finds the code, the args. A family
 * (no one script) draws its first 33 bytes and names the args each cell sets.
 */
export function ScriptByteMap({
  kind,
  script,
  name,
  href,
  codeNote,
  argsNote,
}: ScriptByteMapProps) {
  const subject = kind ? `${kind} script` : 'script';
  if (script && !kind) throw new Error('script byte map: a script needs its role (lock or type)');
  const hashType = script ? HASH_TYPES[script.hashType] : null;
  if (script && !hashType) throw new Error(`${subject}: unknown hash_type ${script.hashType}`);
  if (script && hexBytes(script.codeHash, `${subject} code_hash`).length !== 32)
    throw new Error(`${subject}: code_hash ${script.codeHash} is not 32 bytes`);
  if (script && !href) throw new Error(`${subject}: a script's code hash needs its script page`);
  const args = script ? hexBytes(script.args, `${subject} args`).length : null;
  const total = 33 + (args ?? 0);
  const segments: ScaleSegment[] = [
    {
      key: 'code_hash',
      start: 0,
      end: 32,
      label: `code_hash · 32${name ? ` · ${name}` : ''}`,
      short: '32',
      color: SCRIPT_INKS.codeHash,
      title: `code_hash · [0..32) · 32 B${name ? ` · ${name}` : ''}`,
    },
    {
      key: 'hash_type',
      start: 32,
      end: 33,
      label: '1',
      short: '1',
      color: SCRIPT_INKS.hashType,
      title: `hash_type · [32..33) · 1 B${script ? ` · ${script.hashType}` : ''}`,
    },
    ...(args === null
      ? []
      : [
          {
            key: 'args',
            start: 33,
            end: total,
            label: `args · ${args}`,
            short: String(args),
            color: SCRIPT_INKS.args,
            title: `args · [33..${total}) · ${args} B`,
          },
        ]),
  ];
  return (
    <div className="min-w-0 space-y-3">
      <p className="text-text-dim text-meta">
        {args === null
          ? 'Script · 33 bytes, then the args each cell sets'
          : `Script · ${total} occupied bytes in every cell it ${kind === 'lock' ? 'locks' : 'types'}`}
      </p>
      <ByteScaleMap label={subject} total={total} segments={segments} />
      {script && hashType && href && (
        <dl
          aria-label={`${subject} parts`}
          className="text-small grid grid-cols-[88px_minmax(0,1fr)] gap-x-4 gap-y-1.5"
        >
          <dt style={{ color: SCRIPT_INKS.codeHash }}>code_hash</dt>
          <dd className="m-0 min-w-0 text-[#a0a8be] [overflow-wrap:anywhere]">
            <AppLink href={href}>{script.codeHash}</AppLink>
            {name && (
              <span className="text-text-dim">
                {' · '}
                {/* Responsive A3: a registry name never splits inside a break-anywhere value. */}
                <span className="whitespace-nowrap">{name}</span>
              </span>
            )}
            {codeNote && <span className="text-text-dim"> · {codeNote}</span>}
          </dd>
          <dt style={{ color: SCRIPT_INKS.hashType }}>hash_type</dt>
          <dd className="m-0 min-w-0 text-[#a0a8be]">
            {script.hashType} ({hashType.byte}) · {hashType.words}
          </dd>
          <dt style={{ color: SCRIPT_INKS.args }}>args</dt>
          <dd className="m-0 min-w-0 text-[#a0a8be] [overflow-wrap:anywhere]">
            {args === 0 ? 'empty: no args' : script.args}
            {argsNote && <span className="text-text-dim"> · {argsNote}</span>}
          </dd>
        </dl>
      )}
    </div>
  );
}

function Chain({ from, to, words }: { from: string; to: string; words: string }) {
  return (
    <ol className="flex flex-col gap-1.5 sm:flex-row sm:items-stretch">
      {[
        { title: 'script', body: from },
        { title: to, body: words },
      ].map((tile, i) => (
        <li key={tile.title} className="flex min-w-0 flex-col gap-1.5 sm:flex-1 sm:flex-row">
          {i > 0 && (
            <span aria-hidden="true" className="text-center text-[#343c50] sm:w-5 sm:self-center">
              <span className="sm:hidden">↓</span>
              <span className="hidden sm:inline">→</span>
            </span>
          )}
          <div className="text-small text-text-dim min-w-0 flex-1 rounded-md border border-[#1a1f30] px-3 py-2.5">
            <b className="text-text-bright mb-0.5 block font-medium">{tile.title}</b>
            {tile.body}
          </div>
        </li>
      ))}
    </ol>
  );
}

/**
 * "Two names for the same script" (Address board tier 2): the lock hash and the address are both
 * derived from the script's bytes, as two short chains, each with the name it yields.
 */
export function ScriptNames({ lockHash, address }: { lockHash: string; address: string | null }) {
  const tier = useDetailTier();
  const row = (term: string, value: string) => (
    <dl className="text-small mt-3 grid grid-cols-[88px_minmax(0,1fr)] gap-x-4">
      <dt className="text-text-dim">{term}</dt>
      <dd className="m-0 min-w-0 break-all text-[#a0a8be]">{value}</dd>
    </dl>
  );
  return (
    <section aria-label="Derived names" className="min-w-0">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1.5">
        <h2
          className={cn(
            'font-medium',
            tier === 2 ? 'text-body text-[#a0a8be]' : 'text-text-bright text-section'
          )}
        >
          Two names for the same script
        </h2>
        <span className="text-text-dim text-meta">both derived, neither stored</span>
      </div>
      <div className="mt-3.5">
        <Chain
          from="code_hash · hash_type · args, serialized as molecule"
          to="lock hash"
          words="blake2b-256 of it; what the indexes are keyed by"
        />
        {row('lock hash', lockHash)}
      </div>
      {address !== null && (
        <div className="mt-[18px]">
          <Chain
            from="the same three parts"
            to="address"
            words="bech32m of them, full format; the text at the top of this page"
          />
          {row('address', address)}
        </div>
      )}
      <p className="text-text-dim text-small mt-3.5">
        {address !== null
          ? 'Change one byte of args and both names change; the address is not an account, it is this script spelled out.'
          : 'Change one byte of args and the lock hash changes.'}
      </p>
    </section>
  );
}
