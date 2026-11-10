import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';
import { describe, expect, it } from 'vitest';
import { render, screen } from '../utils/test-utils';
import { Comment } from '@/components/ui/comment';

// Rubbing × Song print, phase 4 (boards voice.js, home.js, notfound.js): a comment is the ghost `#`
// then two or three short lowercase sentences in Courier Prime, dim; the 404 verse is the same row
// at the display size.

const css = readFileSync(resolve(__dirname, '../../app/globals.css'), 'utf-8');

const LINES = [
  'common knowledge is what live cells occupy.',
  'the rest is free, or in the dao.',
  'the three add up to every ckbyte in circulation at #20,969,560.',
];

describe('Comment', () => {
  it('is the .comment row: one paragraph, a line per sentence in order, as written, with no role', () => {
    const { container } = render(<Comment lines={LINES} />);
    const row = container.firstElementChild as HTMLElement;
    expect(row.tagName).toBe('DIV');
    expect(row).toHaveClass('comment');
    expect(row).not.toHaveClass('comment-lg');
    expect(row).not.toHaveAttribute('role');
    // the `#` is the row's ::before (CSS), so it is not in the text a reader is given
    expect(row.textContent!.startsWith('#')).toBe(false);
    const paragraph = row.firstElementChild as HTMLElement;
    expect(paragraph.tagName).toBe('P');
    expect(row.children).toHaveLength(1);
    const lines = Array.from(paragraph.querySelectorAll('[data-comment-line]'));
    expect(lines.map((l) => l.textContent)).toEqual(LINES);
    for (const line of lines) expect(line).toHaveClass('block');
    // nothing changes the case: no class sets it, the text is the text given
    expect(row.innerHTML).not.toMatch(/lowercase|uppercase|capitalize/);
    // a reader hears the sentences apart, not run together
    expect(paragraph).toHaveTextContent(LINES.join(' '));
  });

  it('is the page title at the display size when it is the 404 verse', () => {
    render(
      <Comment
        as="h1"
        size="lg"
        lines={[
          'no page at this path.',
          'the chain was not asked; nothing on it is lost.',
          "honey badger don't care.",
        ]}
      />
    );
    const h1 = screen.getByRole('heading', {
      level: 1,
      name: "no page at this path. the chain was not asked; nothing on it is lost. honey badger don't care.",
    });
    expect(h1).toHaveClass('comment', 'comment-lg');
    expect(h1.firstElementChild!.tagName).toBe('SPAN');
    expect(h1.querySelectorAll('[data-comment-line]')).toHaveLength(3);
  });

  it('refuses a comment with no line or a blank one', () => {
    expect(() => render(<Comment lines={[]} />)).toThrow('at least one line');
    expect(() => render(<Comment lines={['one.', '  ']} />)).toThrow('line 2 is blank');
  });

  it('.comment-lg: 21 px from 640 (18 on a phone), the # at 16, the three inks of the verse', () => {
    const lg = css.match(/\n\.comment-lg\s*\{([^}]*)\}/)!;
    expect(lg[1]).toContain('gap: 16px');
    expect(lg[1]).toContain('font-size: 18px');
    expect(lg[1]).toContain('line-height: 1.5');
    expect(css).toMatch(
      /@media \(min-width: 640px\)\s*\{\s*\.comment-lg\s*\{\s*font-size: 21px;\s*\}\s*\}/
    );
    expect(css).toMatch(/\n\.comment-lg::before\s*\{\s*font-size: 16px;\s*\}/);
    const body = css.match(/\n\.comment-lg > \*\s*\{([^}]*)\}/)!;
    expect(body[1]).toContain('flex-direction: column');
    expect(body[1]).toContain('color: var(--c-text)');
    expect(css).toMatch(
      /\n\.comment-lg > \* > :first-child\s*\{\s*color: var\(--c-text-bright\);\s*\}/
    );
    expect(css).toMatch(
      /\n\.comment-lg > \* > :last-child:not\(:first-child\)\s*\{\s*color: var\(--c-jade\);\s*\}/
    );
  });
});
