// Consistency plan C1: every page names itself in the tab — `<name> · <kind> · CKBadger` — through
// one hook that the shared headers call and that restores the previous title on unmount.
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import type { ReactNode } from 'react';
import { render } from '@testing-library/react';
import {
  SITE_TITLE,
  documentTitleParts,
  formatDocumentTitle,
  titleAddress,
  titleHash,
  useDocumentTitle,
} from '@/lib/use-document-title';

function Titled({ parts, children }: { parts: readonly string[]; children?: ReactNode }) {
  useDocumentTitle(parts);
  return <>{children}</>;
}

describe('useDocumentTitle (C1)', () => {
  const slogan = 'CKBadger — Local-first CKB-native Explorer';
  beforeEach(() => {
    document.title = slogan;
  });
  afterEach(() => {
    document.title = slogan;
  });

  it('joins the parts with the site name after a middle dot', () => {
    expect(SITE_TITLE).toBe('CKBadger');
    expect(formatDocumentTitle(['0xdcd1…334d', 'transaction'])).toBe(
      '0xdcd1…334d · transaction · CKBadger'
    );
    expect(formatDocumentTitle(['Blocks'])).toBe('Blocks · CKBadger');
  });

  it('sets document.title while mounted and restores the previous title on unmount', () => {
    const view = render(<Titled parts={['#20,894,843', 'block']} />);
    expect(document.title).toBe('#20,894,843 · block · CKBadger');
    view.unmount();
    expect(document.title).toBe(slogan);
  });

  it('follows the parts when they change (a page whose name arrives with its data)', () => {
    const view = render(<Titled parts={['Token']} />);
    expect(document.title).toBe('Token · CKBadger');
    view.rerender(<Titled parts={['RGB++', 'token']} />);
    expect(document.title).toBe('RGB++ · token · CKBadger');
    view.unmount();
    expect(document.title).toBe(slogan);
  });

  it('ignores empty parts', () => {
    render(<Titled parts={['', 'Charts', '']} />);
    expect(document.title).toBe('Charts · CKBadger');
  });

  it('reads a string heading as the name, and refuses a non-string heading without parts', () => {
    expect(documentTitleParts('Blocks', undefined)).toEqual(['Blocks']);
    expect(documentTitleParts('RGB++', undefined, 'token')).toEqual(['RGB++', 'token']);
    expect(documentTitleParts(<b>x</b>, ['21523.bit', 'identity'])).toEqual([
      '21523.bit',
      'identity',
    ]);
    expect(() => documentTitleParts(<b>x</b>, undefined)).toThrow(/documentTitle/);
  });

  it('lets the page win over a header inside it, and leaves the old title after both unmount', () => {
    const view = render(
      <Titled parts={['0xdcd1…334d', 'transaction']}>
        <Titled parts={['Transaction']} />
      </Titled>
    );
    expect(document.title).toBe('0xdcd1…334d · transaction · CKBadger');
    view.unmount();
    expect(document.title).toBe(slogan);
  });

  it('shortens hashes and addresses the way the tab names them', () => {
    expect(titleHash('0xdcd14a81fb8897d10280153792b4fe99ea9dc991c7d8ca5ac7700f72e91a334d')).toBe(
      '0xdcd1…334d'
    );
    expect(
      titleAddress(
        'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfs6k9ygrwtdrn828l4a04ns9fnes960jg3x8k26'
      )
    ).toBe('ckb1qzda…3x8k26');
    expect(() => titleHash('Default Lock')).toThrow(/not a hash/);
    expect(() => titleAddress('0xdcd1')).toThrow(/not a CKB address/);
  });
});
