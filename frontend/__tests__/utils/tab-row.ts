import { fireEvent, screen, waitFor, within } from '@testing-library/react';

/**
 * Opens the view named `name` of a tab row in the form its line gives it (the lead's final round,
 * item 4): a tab where the row holds its line, else the row's narrow form, the Select after
 * "showing". A page test that stubs every measured box to one width (358 px under a viewport from
 * 640, where a command row is set at the section step) gives its row a line that cannot hold the
 * names, which the row now answers with its Select; the test is about the list the view opens.
 */
export async function chooseView(name: RegExp): Promise<void> {
  const form = await waitFor(() => {
    const found =
      screen.queryByRole('tab', { name }) ?? screen.queryByRole('combobox', { name: 'showing' });
    if (found === null) throw new Error(`no tab row with a view ${name} yet`);
    return found;
  });
  if (form.getAttribute('role') === 'tab') {
    fireEvent.click(form);
    return;
  }
  const option = within(form)
    .getAllByRole('option')
    .find((o) => name.test(o.textContent ?? ''));
  if (option === undefined) throw new Error(`no view ${name} in the tab row's Select`);
  fireEvent.change(form, { target: { value: (option as HTMLOptionElement).value } });
}
