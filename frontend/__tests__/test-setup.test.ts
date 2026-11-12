import { getConfig, screen } from '@testing-library/react';

// __tests__/setup.ts sets how long a wait waits; it must reach the Testing Library that the tests'
// screen.findBy* and waitFor use (the one @testing-library/react re-exports), not another copy.
describe('the async wait of the test setup', () => {
  it('is the deliberate 10 s, not the 1 s default', () => {
    expect(getConfig().asyncUtilTimeout).toBe(10_000);
  });

  it('lets a findBy* outwait the old 1 s default', async () => {
    const late = document.createElement('p');
    late.textContent = 'arrived after the old default';
    setTimeout(() => document.body.append(late), 1_100);
    try {
      expect(await screen.findByText('arrived after the old default')).toBe(late);
    } finally {
      late.remove();
    }
  });
});
