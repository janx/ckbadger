import { fireEvent, render, screen, within } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { ThemeSegments, ThemeSwitcher } from '@/components/layout/theme-switcher';
import { resetThemeForTests, THEME_STORAGE_KEY } from '@/lib/theme';

describe('ThemeSwitcher (the header menu)', () => {
  beforeEach(() => {
    localStorage.clear();
    resetThemeForTests();
  });
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  const button = () => screen.getByRole('button', { name: /^Theme: / });

  it('is a menu button naming the preference in force, closed until asked', () => {
    render(<ThemeSwitcher />);
    expect(button()).toHaveAccessibleName('Theme: Dark');
    expect(button()).toHaveAttribute('aria-haspopup', 'menu');
    expect(button()).toHaveAttribute('aria-expanded', 'false');
    expect(button().querySelector('[data-glyph]')).toHaveAttribute('data-glyph', 'moon');
    expect(screen.queryByRole('menu')).toBeNull();
  });

  it('opens on click to three radio rows, the one in force checked, System naming the OS scheme', () => {
    render(<ThemeSwitcher />);
    fireEvent.click(button(), { detail: 1 });
    const menu = screen.getByRole('menu', { name: 'Theme' });
    const rows = within(menu).getAllByRole('menuitemradio');
    expect(rows.map((r) => r.textContent)).toEqual(['Dark', 'Light', 'Systemdark']);
    expect(rows.map((r) => r.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    expect(button()).toHaveAttribute('aria-expanded', 'true');
  });

  it('chooses a theme: stored, applied to <html>, the button renamed, the menu closed', () => {
    render(<ThemeSwitcher />);
    fireEvent.click(button(), { detail: 1 });
    fireEvent.click(screen.getByRole('menuitemradio', { name: 'Light' }));
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(button()).toHaveAccessibleName('Theme: Light');
    expect(button().querySelector('[data-glyph]')).toHaveAttribute('data-glyph', 'sun');
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(button());
    fireEvent.click(button(), { detail: 1 });
    fireEvent.click(screen.getByRole('menuitemradio', { name: /^System/ }));
    expect(button()).toHaveAccessibleName('Theme: System');
    expect(button().querySelector('[data-glyph]')).toHaveAttribute('data-glyph', 'system');
    // the OS scheme in the test is dark
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('works from the keyboard as a menu button: ↓ opens on the first row, Escape closes back', () => {
    render(<ThemeSwitcher />);
    button().focus();
    fireEvent.keyDown(button(), { key: 'ArrowDown' });
    const rows = screen.getAllByRole('menuitemradio');
    expect(document.activeElement).toBe(rows[0]);
    fireEvent.keyDown(rows[0], { key: 'ArrowDown' });
    expect(document.activeElement).toBe(rows[1]);
    fireEvent.keyDown(rows[1], { key: 'End' });
    expect(document.activeElement).toBe(rows[2]);
    fireEvent.keyDown(rows[2], { key: 'ArrowDown' });
    expect(document.activeElement).toBe(rows[0]);
    fireEvent.keyDown(rows[0], { key: 'Escape' });
    expect(screen.queryByRole('menu')).toBeNull();
    expect(document.activeElement).toBe(button());
    // Enter / Space arrive as a click without a pointer: the row in force takes focus
    fireEvent.click(button(), { detail: 0 });
    expect(document.activeElement).toBe(screen.getByRole('menuitemradio', { name: 'Dark' }));
  });

  it('opens on hover and closes when the pointer leaves, unless a click pinned it', () => {
    render(<ThemeSwitcher />);
    const wrapper = screen.getByTestId('theme-switcher');
    fireEvent.mouseEnter(wrapper);
    expect(screen.getByRole('menu')).toBeInTheDocument();
    fireEvent.mouseLeave(wrapper);
    expect(screen.queryByRole('menu')).toBeNull();
    fireEvent.mouseEnter(wrapper);
    fireEvent.click(button(), { detail: 1 });
    fireEvent.mouseLeave(wrapper);
    expect(screen.getByRole('menu')).toBeInTheDocument();
    // a click outside closes a pinned menu
    fireEvent.pointerDown(document.body);
    expect(screen.queryByRole('menu')).toBeNull();
  });
});

describe('ThemeSegments (the phone menu row)', () => {
  beforeEach(() => {
    localStorage.clear();
    resetThemeForTests();
  });
  afterEach(() => {
    localStorage.clear();
    resetThemeForTests();
    document.documentElement.setAttribute('data-theme', 'dark');
  });

  it('is a radio group of the three preferences; a choice applies and reports back', () => {
    const onChoose = vi.fn();
    render(<ThemeSegments onChoose={onChoose} />);
    const group = screen.getByRole('radiogroup', { name: 'Theme' });
    const radios = within(group).getAllByRole('radio');
    expect(radios.map((r) => r.getAttribute('aria-checked'))).toEqual(['true', 'false', 'false']);
    expect(radios[2]).toHaveTextContent('System(dark)');
    fireEvent.click(radios[1]);
    expect(onChoose).toHaveBeenCalledTimes(1);
    expect(localStorage.getItem(THEME_STORAGE_KEY)).toBe('light');
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(radios.map((r) => r.getAttribute('aria-checked'))).toEqual(['false', 'true', 'false']);
  });
});
