// Lane P2's follow-up (items 8 and 9): a whole id in the evidence tier of an object or an identity
// is set as a head's id is (HeadId): one line where its measured line holds it, else two halves,
// `0x` and its digits split even, each one unit; never a box that scrolls sideways and never a
// unit wider than the line. At the facts' 13 px a 32-byte hash is 66 × 7.8 = 514.8 px and a half
// 34 × 7.8 = 265.2; a .bit account id, 20 bytes, is 42 × 7.8 = 327.6 px, wider than the 288 px a
// 320 px screen gives the tier's value column (the dotbit page ran 24 px past it, 327.6 + 16
// rounded up less 320) and narrower than 328 (360) or 358 (390): one line from 360, two halves of
// 22 characters (171.6 px) at 320.
import { render, screen, within } from '@/__tests__/utils/test-utils';
import { ItemEvidence } from '@/components/asset/item-evidence';
import { DotbitAccountData } from '@/components/identity/identity-evidence';
import { NamedParty } from '@/components/asset/identity-document';
import type { ItemProtocol } from '@/lib/api';
import {
  didProtocolProducer,
  dotbitV4ProtocolProducer,
} from '@/__tests__/msw/fixtures/item-producer';

/** No element of `root` is a sideways scroller or holds a whole id on one unbreakable line. */
function expectNoScroller(root: HTMLElement) {
  for (const el of Array.from(root.querySelectorAll('[class]')))
    expect(el.getAttribute('class')).not.toMatch(/\boverflow-x-(auto|scroll)\b/);
}

describe('whole ids in the evidence tier (items 8, 9)', () => {
  it('sets the raw item evidence’s out point as a head id, linked to its cell', () => {
    const protocol = structuredClone(didProtocolProducer) as ItemProtocol;
    render(<ItemEvidence protocol={protocol} />);
    const cell = protocol.versions.at(-1)!.cell;
    const section = screen.getByRole('region', { name: 'Raw item evidence' });
    const id = section.querySelector<HTMLElement>(`[data-head-id][title="${cell.txHash}"]`)!;
    expect(id).not.toBeNull();
    expect(id.closest('a')).toHaveAttribute(
      'href',
      `/mainnet/cell/${cell.txHash}-${cell.outputIndex}`
    );
    expect(section).toHaveTextContent(`Outpoint · output ${cell.outputIndex} of`);
    expectNoScroller(section);
  });

  it('sets a .bit account’s id as a head id, so a 320 px screen holds it in two halves', () => {
    render(<DotbitAccountData protocol={dotbitV4ProtocolProducer as never} />);
    const section = screen.getByRole('region', { name: 'Account data, from the witness' });
    const row = section.querySelector<HTMLElement>('[data-row="account"]')!;
    const accountId = (
      dotbitV4ProtocolProducer as { versions: { decoded: { accountId: string } }[] }
    ).versions.at(-1)!.decoded.accountId;
    const id = row.querySelector<HTMLElement>('[data-head-id]')!;
    expect(id).toHaveAttribute('title', accountId);
    // no unbreakable box around it: the field that held it on one line is gone
    expect(id.closest('.whitespace-nowrap')).toBeNull();
    expect(within(row).getByText('next')).toBeInTheDocument();
    expectNoScroller(section);
  });

  it('sets a named lock-hash prefix as a head id with its copy after it', () => {
    const prefix = '0x' + '69'.repeat(20);
    render(
      <dl>
        <NamedParty
          label="owner"
          party={{ hashPrefix: prefix, lockHash: null, address: null, scriptName: null }}
        />
      </dl>
    );
    const id = document.querySelector<HTMLElement>('[data-head-id]')!;
    expect(id).toHaveAttribute('title', prefix);
    expect(screen.getByRole('button', { name: 'copy lock hash prefix' })).toBeInTheDocument();
    expectNoScroller(document.body);
  });
});
