import { render, screen } from '@/__tests__/utils/test-utils';
import { CellVersions } from '@/components/asset/cell-versions';
import { dotbitV4ProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';

it('prints each version capacity once with its unit, never "CKB CKB" (OB-3, ID-C3)', () => {
  const cells = dotbitV4ProtocolProducer.versions.map((version) => version.cell);
  render(<CellVersions history={{ data: cells, total: cells.length }} subject="alice.bit" />);
  const list = screen.getByLabelText('alice.bit cell versions');
  expect(list.textContent).not.toMatch(/CKB\s*CKB/);
  expect(list).toHaveTextContent('CKB');
});
