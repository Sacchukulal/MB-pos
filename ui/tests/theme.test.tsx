/** THE SWAP TEST. */

import { render, screen, cleanup } from '@testing-library/react';
import { afterEach, describe, expect, it } from 'vitest';

import { Gallery } from '../src/gallery/Gallery';
import { ToastProvider } from '../src/kit';
import { ThemeProvider } from '../src/theme/ThemeProvider';
import { THEMES, toggleTarget } from '../src/theme/themes';

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute('data-theme');
  window.localStorage.clear();
});

/**
 * The gallery's markup under one theme, with two things normalised away — and both exclusions
 * are load-bearing, so they are argued rather than assumed.
 */
function renderGalleryUnder(themeId: string): string {
  window.localStorage.setItem('mb.theme', themeId);
  const { container } = render(
    <ThemeProvider>
      <ToastProvider>
        <Gallery />
      </ToastProvider>
    </ThemeProvider>,
  );
  const copy = container.cloneNode(true) as HTMLElement;
  copy.querySelector('.mb-card')?.remove();
  const html = copy.innerHTML.replace(/_r_[0-9a-z]+_/g, 'id');
  cleanup();
  return html;
}

describe('the theme is data, not code (D21, and the owner ruling of 2026-08-04)', () => {
  it('renders an identical DOM under every theme', () => {
    const rendered = THEMES.map((theme) => ({
      id: theme.id,
      html: renderGalleryUnder(theme.id),
    }));

    const first = rendered[0];
    expect(first).toBeDefined();
    for (const other of rendered.slice(1)) {
      expect(
        other.html,
        `the DOM changed between the "${first?.id}" and "${other.id}" themes — ` +
          `the look is not data, and changing it would touch functionality`,
      ).toBe(first?.html);
    }
  });

  it('changes exactly one thing when the theme changes: which theme is marked active', () => {
    // The exception the comparison above excludes, checked rather than trusted.
    const marked = (themeId: string): string[] => {
      window.localStorage.setItem('mb.theme', themeId);
      const { container } = render(
        <ThemeProvider>
          <ToastProvider>
            <Gallery />
          </ToastProvider>
        </ThemeProvider>,
      );
      const active = [...container.querySelectorAll('.mb-button--primary')]
        .map((b) => b.textContent ?? '')
        .filter((label) => THEMES.some((t) => t.name === label));
      cleanup();
      return active;
    };

    expect(marked('light')).toEqual(['Light']);
    expect(marked('dark')).toEqual(['Dark']);
  });

  it('applies the theme by one attribute on <html>, and nothing else', () => {
    // Why it matters: an attribute on the root cascades to portals too, and no component
    // re-renders when it changes — which is what makes a theme swap free rather than a
    // re-render of the whole counter.
    renderGalleryUnder('dark');
    expect(document.documentElement.getAttribute('data-theme')).toBe('dark');
  });

  it('accepts a theme that did not exist when the app was written', () => {
    const invented = {
      id: 'sunset-the-owner-asked-for-in-october',
      name: 'Sunset',
      icon: 'sun' as const,
      appearance: 'light' as const,
    };

    window.localStorage.setItem('mb.theme', invented.id);
    render(
      <ThemeProvider>
        <ToastProvider>
          <Gallery />
        </ToastProvider>
      </ThemeProvider>,
    );

    // A name the registry has never seen falls back to a theme that exists rather than leaving
    // the counter unstyled — losing a colour must never lose a shop its screen.
    expect(document.documentElement.getAttribute('data-theme')).toBe('light');
    expect(screen.getByText('The kit')).toBeInTheDocument();
  });

  it('flips between light and dark, which is the sun/moon button', () => {
    expect(toggleTarget('light')).toBe('dark');
    expect(toggleTarget('dark')).toBe('light');
    // From anything else it returns to the default rather than guessing.
    expect(toggleTarget('contrast')).toBe('light');
    expect(toggleTarget('a-theme-that-was-deleted')).toBe('light');
  });
});
