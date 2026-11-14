import { describe, expect, it } from 'vitest';
import { render, screen } from '@testing-library/react';
import { NoBreak } from '@/components/ui/no-break';
import { HexCode } from '@/components/ui/hex-code';
import { Hash } from '@/components/ui/hash';
import { Address } from '@/components/ui/address';
import { OutpointIndex } from '@/components/ui/outpoint-index';
import { Utc8Date, Utc8DateTime } from '@/components/ui/utc8-date';
import { ExactCkb } from '@/components/activity-event-row';
import { formatCount } from '@/lib/format-count';

/**
 * U-16, PA-05, PB-05 (UI review 2026-11-14): `docs/RESPONSIVE_LAYOUT.md`, "Non-breaking units": a
 * shortened hash, an address, an out point with its index, a date, a date with its time and a
 * number with its unit are each one token, never broken inside. Seen broken on about twenty pages
 * ("deployed in 0xe2fb19…" / "73ff5c", "since 2024-" / "08-29", "78,647,937" / "cells"). Each
 * kind is made unbreakable in its one renderer, by the one `NoBreak` piece.
 */
const HASH = '0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef';
const ADDRESS = 'ckb1qzda0cr08m85hc8jlnfp3zer7xulejywt49kt2rr0vthywaa50xwsqfqyerjauq9ze4k6kmfrz';

/** The nearest element (itself or an ancestor) that lays its text on one line. */
const nowrapOf = (el: Element | null): Element | null =>
  el === null ? null : el.classList.contains('whitespace-nowrap') ? el : nowrapOf(el.parentElement);

describe('NoBreak, the one piece that keeps a unit whole', () => {
  it('lays its content on one line and names itself', () => {
    render(
      <p>
        since <NoBreak>2024-08-29</NoBreak>
      </p>
    );
    const unit = screen.getByText('2024-08-29');
    expect(unit.tagName).toBe('SPAN');
    expect(unit).toHaveClass('whitespace-nowrap');
    expect(unit).toHaveAttribute('data-nobreak');
  });
});

describe('a shortened id is one unit', () => {
  it('HexCode and Hash keep a shortened hash whole', () => {
    render(<HexCode value={HASH} startChars={8} endChars={6} copyable={false} />);
    const short = screen.getByText('0x123456...abcdef');
    expect(nowrapOf(short)).not.toBeNull();
    render(<Hash hash={HASH} copyable={false} />);
    expect(nowrapOf(screen.getByText('0x12345678...90abcdef'))).not.toBeNull();
  });

  it('Address keeps a shortened address whole', () => {
    render(<Address address={ADDRESS} />);
    const short = `${ADDRESS.slice(0, 12)}...${ADDRESS.slice(-8)}`;
    expect(nowrapOf(screen.getByText(short))).not.toBeNull();
  });

  it('an out point keeps its index with its hash', () => {
    const { container } = render(
      <span>
        <HexCode value={HASH} startChars={10} endChars={6} copyable={false} />
        <OutpointIndex index={0} />
      </span>
    );
    const index = container.querySelector('[data-outpoint-index]');
    expect(index).toHaveAttribute('data-nobreak');
    expect(index).toHaveClass('whitespace-nowrap');
  });
});

describe('a full hash is one token: whole on its line, else scrolled inside its own box', () => {
  it('HexCode writes a full value on one line in a box no wider than its column', () => {
    render(<HexCode value={HASH} truncate={false} copyable={false} />);
    const full = screen.getByText(HASH);
    expect(full).toHaveAttribute('data-full-value');
    // one line, never wrapped in the middle …
    expect(full).toHaveClass('whitespace-nowrap');
    expect(full).not.toHaveClass('break-all');
    // … in its own box that is never wider than its column and scrolls sideways when it must
    expect(full).toHaveClass('inline-block', 'max-w-full', 'overflow-x-auto', 'align-top');
  });

  it('Hash passes the same behaviour on', () => {
    render(<Hash hash={HASH} truncate={false} copyable={false} />);
    expect(screen.getByText(HASH)).toHaveAttribute('data-full-value');
  });
});

describe('a date and a date with its time are one unit', () => {
  it('Utc8Date never breaks at the hyphens', () => {
    render(<Utc8Date timestamp={Date.UTC(2024, 7, 28, 16, 30)} />);
    const date = screen.getByText('2024-08-29');
    expect(date.tagName).toBe('TIME');
    expect(nowrapOf(date)).not.toBeNull();
  });

  it('Utc8DateTime writes the date, the time and the zone as one unit', () => {
    render(<Utc8DateTime timestamp={Date.UTC(2026, 9, 3, 0, 31, 52)} />);
    const at = screen.getByText('2026-10-03 08:31:52 UTC+8');
    expect(at.tagName).toBe('TIME');
    expect(nowrapOf(at)).not.toBeNull();
  });
});

describe('a number and its unit are one unit', () => {
  it('formatCount joins the count and its noun with a no-break space', () => {
    expect(formatCount(78_647_937, 'cell')).toBe('78,647,937 cells');
    expect(formatCount(1, 'binary', 'binaries')).toBe('1 binary');
  });

  it('ExactCkb keeps its unit with its digits, also where it may wrap', () => {
    const { container } = render(
      <ExactCkb shannons={BigInt('86460521911728318')} signed={false} tone="plain" wrap />
    );
    const unit = screen.getByText('CKB');
    const group = nowrapOf(unit) as HTMLElement;
    expect(group).not.toBeNull();
    // the unit never stands alone at a line's start: it shares a group with the last digits
    expect(group.textContent).toMatch(/\.11728318\s?CKB$/);
    expect(container.textContent).toBe('864,605,219.11728318 CKB');
  });
});
