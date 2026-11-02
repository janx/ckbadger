import { describe, expect, it } from 'vitest';
import { render } from '@/__tests__/utils/test-utils';
import { TintedJson } from '@/components/ui/tinted-json';

describe('TintedJson (lane E)', () => {
  it('prints the exact text with its keys and values tinted', () => {
    const text = JSON.stringify(
      { dob: { ver: 0, pattern: [['Entity', 'number', 75, 8, 'raw']] }, ok: true },
      null,
      2
    );
    const { container } = render(<TintedJson text={text} />);
    const pre = container.querySelector('pre')!;
    expect(pre.textContent).toBe(text);
    expect([...pre.querySelectorAll('[data-json="key"]')].map((k) => k.textContent)).toEqual([
      '"dob"',
      '"ver"',
      '"pattern"',
      '"ok"',
    ]);
    expect([...pre.querySelectorAll('[data-json="value"]')].map((k) => k.textContent)).toEqual([
      '0',
      '"Entity"',
      '"number"',
      '75',
      '8',
      '"raw"',
      'true',
    ]);
    expect(pre).toHaveClass('whitespace-pre-wrap', 'break-all', 'max-w-full', 'text-meta');
  });
});
