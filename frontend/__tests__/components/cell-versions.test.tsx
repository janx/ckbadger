import { render, screen } from '@/__tests__/utils/test-utils';
import { CellVersions } from '@/components/asset/cell-versions';
import { dotbitV4ProtocolProducer } from '@/__tests__/msw/fixtures/item-producer';
import { expectPrompt } from '@/__tests__/utils/prompt-heading';

it('prints each version capacity once with its unit, never "CKB CKB" (OB-3, ID-C3)', () => {
  const cells = dotbitV4ProtocolProducer.versions.map((version) => version.cell);
  render(<CellVersions history={{ data: cells, total: cells.length }} subject="alice.bit" />);
  const list = screen.getByLabelText('alice.bit cell versions');
  expect(list.textContent).not.toMatch(/CKB\s*CKB/);
  expect(list).toHaveTextContent('CKB');
});

// Rubbing × Song print, phase 5 (D13, D12): the default section is the versions, run on their
// count; a caller that names the section (the boards' "CKB held") gives the heading whole.
it('runs "cell versions" on the count of versions, and a named section on nothing', () => {
  const cells = dotbitV4ProtocolProducer.versions.map((version) => version.cell);
  const history = { data: cells, total: cells.length };
  const { unmount } = render(<CellVersions history={history} subject="alice.bit" />);
  expectPrompt(
    screen.getByRole('heading', { name: `Cell versions ${cells.length}` }),
    'Cell versions',
    String(cells.length)
  );
  // the note stays the row's right end, outside the heading
  expect(screen.getByText('Every canonical cell version · oldest first')).not.toBe(
    screen.getByRole('heading')
  );
  expect(screen.getByRole('heading')).not.toHaveTextContent('oldest first');
  unmount();
  render(
    <CellVersions
      history={history}
      subject="alice.bit"
      title="CKB held"
      actions={<span>1 CKB per byte</span>}
    />
  );
  expectPrompt(screen.getByRole('heading', { name: 'CKB held' }), 'CKB held');
  expect(screen.getByRole('heading').parentElement).toHaveTextContent('1 CKB per byte');
});
