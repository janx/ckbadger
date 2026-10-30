import { render, screen } from '@/__tests__/utils/test-utils';
import { MnftPermissions, NamedParty } from '@/components/asset/identity-document';
import { mnftProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';
// The identity read-out these tests covered moved to components/identity/* (F2 identities,
// round 2); its invariants are tested there (identity-sections / identity-evidence tests).
it('links only a uniquely resolved named party', () => {
  render(
    <dl>
      <NamedParty
        label="Manager"
        party={{
          hashPrefix: '0x11',
          lockHash: '0x22',
          address: 'ckb1manager',
          scriptName: 'JoyID',
        }}
      />
    </dl>
  );
  expect(screen.getByRole('link')).toHaveAttribute('href', '/mainnet/address/ckb1manager');
  expect(screen.getByText('JoyID')).toBeInTheDocument();
});
it('uses official configure permissions and keeps locked state independent', () => {
  const d = mnftProtocolProducer.versions[0].decoded;
  render(
    <MnftPermissions
      permissions={d.permissions}
      configure={d.configure}
      state={{ claimed: d.claimed, locked: d.locked, value: d.state }}
    />
  );
  const panel = screen.getByLabelText('mNFT permissions');
  expect(panel).toHaveTextContent('Configure 0xc0');
  expect(panel).toHaveTextContent('Destroy before claimForbidden');
  expect(panel).toHaveTextContent('Transfer after claimAllowed');
  expect(panel).toHaveTextContent('locked state prevents transfer');
});
