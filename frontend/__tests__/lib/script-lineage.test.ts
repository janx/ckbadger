import { describe, expect, it } from 'vitest';
import fixture from '@/__tests__/fixtures/scripts/joyid-family-detail.json';
import type { ScriptFamilyDetail, ScriptHistoryMarkerApi } from '@/lib/api';
import {
  codeHolders,
  deploymentOnDay,
  holderName,
  holderParts,
  orderedVersions,
  signerOf,
  signingText,
  typeLineage,
  versionNumbers,
} from '@/lib/script-lineage';
import { shortenHash } from '@/lib/shorten';

const family = fixture.family as unknown as ScriptFamilyDetail;
const markers = family.history as ScriptHistoryMarkerApi[];

describe('script lineage (JoyID, five binaries under one Type ID)', () => {
  it('numbers versions in deployment order', () => {
    const numbers = versionNumbers(family);
    expect(orderedVersions(family).map((v) => numbers.get(v.versionHash))).toEqual([1, 2, 3, 4, 5]);
    expect(numbers.get('0x4af7d832e7c328aa4ad6f2deb669743446aea7b9a2f0c25646897501ac4f1ebe')).toBe(
      5
    );
  });

  it('reads the single Type ID lineage and refuses to pick one of several', () => {
    expect(typeLineage(markers)!.map((m) => m.blockNumber)).toEqual([
      11132140, 11133523, 17117990, 17119035, 17894900,
    ]);
    const second = markers.map((m) => ({ ...m, referenceHash: `0x${'ee'.repeat(32)}` }));
    expect(typeLineage([...markers, ...second])).toBeNull();
  });

  it('pairs each upgrade with the holder that signed it', () => {
    const lineage = typeLineage(markers)!;
    expect(signerOf(lineage[0], markers)).toBeNull();
    expect(signerOf(lineage[2], markers)!.lockArgs).toBe(
      '0xd1dbde9214a1cb1e377c9b643ae5c680de0cb95c'
    );
    expect(() =>
      signerOf(lineage[2], [
        ...markers,
        {
          ...markers.find(
            (m) => m.event === 'consumed' && m.eventTxHash === lineage[2].eventTxHash
          )!,
          codeCellOutputIndex: 9,
        },
      ])
    ).toThrow(/consumed 2 code cells/);
  });

  it('lists the three multisigs, what each signed and that the last never signed', () => {
    const lineage = typeLineage(markers)!;
    const holders = codeHolders(lineage, markers, versionNumbers(family));
    expect(holders.map((h) => shortenHash(h.lockArgs, 'tight'))).toEqual([
      '0xd1db…b95c',
      '0x07a2…68ca',
      '0x6598…d696',
    ]);
    expect(holders.map((h) => h.signed)).toEqual([[2, 3], [4, 5], []]);
    expect(holders.map((h) => signingText(h))).toEqual([
      '3 of 5 signatures',
      '3 of 5 signatures',
      'signers not on chain yet',
    ]);
    expect(holders[2].neverSigned).toBe(true);
    expect(holders[0].until!.blockNumber).toBe(17117990);
    expect(holders[2].until).toBeNull();
    // U-09 (re-pinned): the name's lock args at the size of where it stands
    expect(holderName(holders[0].custody, 'tight')).toBe('multisig 0xd1db…b95c');
    expect(holderName(holders[0].custody, 'row')).toBe('multisig 0xd1dbde92…0cb95c');
  });

  it('says which deployment was in force on a chart day', () => {
    const lineage = typeLineage(markers)!;
    expect(deploymentOnDay(lineage, '2023-10-06')).toBeNull();
    expect(deploymentOnDay(lineage, '2024-06-01')!.blockNumber).toBe(11133523);
    expect(deploymentOnDay(lineage, '2026-10-29')!.blockNumber).toBe(17894900);
  });

  // A4 Chrome F2: a holder is named either by a kind and its id ("multisig 0xd1db…b95c", a label
  // with its id, one unit) or by a phrase ("an unspendable lock", words a line may break). The
  // name says which where the kind is known, so a renderer keeps only the id form whole.
  it('names a holder in parts: a kind with its id, or a phrase with none', () => {
    const custody = markers.at(-1)!.custody;
    expect(holderParts(custody, 'tight')).toEqual({ label: 'multisig', id: '0x6598…d696' });
    expect(holderParts({ ...custody, class: 'one_key' }, 'tight')).toEqual({
      label: 'one key',
      id: '0x6598…d696',
    });
    expect(holderParts({ ...custody, class: 'unspendable' }, 'tight')).toEqual({
      label: 'an unspendable lock',
      id: null,
    });
    expect(holderParts({ ...custody, class: 'fixed' }, 'tight')).toEqual({
      label: 'no one (pinned by data hash)',
      id: null,
    });
    expect(holderParts({ ...custody, class: 'other' as never }, 'tight')).toEqual({
      label: 'lock',
      id: shortenHash(custody.lockCodeHash!, 'tight'),
    });
    expect(
      holderParts({ ...custody, class: 'other' as never, lockCodeHash: null }, 'tight')
    ).toEqual({ label: 'an unrecorded holder', id: null });
    // the one string form joins the two with a space
    expect(holderName(custody, 'tight')).toBe('multisig 0x6598…d696');
    expect(holderName({ ...custody, class: 'unspendable' }, 'tight')).toBe('an unspendable lock');
  });
});
