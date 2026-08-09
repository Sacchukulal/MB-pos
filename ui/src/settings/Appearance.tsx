/**
 * **How it looks** — scope 13.12, and the owner's ruling of 2026-08-04.
 *
 * > *"Design a central theme system so that in future it can be changed easily
 * > with my suggestion WITHOUT TOUCHING ANY FUNCTIONALITY OF THE APP."*
 *
 * So this screen holds **no colour and no palette**. It lists what
 * `themes.ts` declares and applies it through `ThemeProvider`, which sets one
 * attribute on `<html>`. Adding a theme is still: one block in `tokens.css`,
 * one line in `themes.ts`, and nothing here — a test in `theme.test.tsx` adds
 * a throwaway theme that way and asserts it renders.
 *
 * # Why the theme is not a setting like the others
 *
 * It is not the shop's, it is the **machine's**: it has to be applied before
 * the first paint and it has to work when the database will not open. So it
 * lives in `AppConfig` beside the window size, and only the LANGUAGE — which
 * is on the receipt — comes down the catalogue.
 */

import { Card, Radio, SectionHeader } from '../kit';
import { call, inApp } from '../ipc/call';
import { useTheme } from '../theme/ThemeProvider';
import { TEXT_SIZES, THEMES } from '../theme/themes';

export function Appearance() {
  const { theme, textSize, setTheme, setTextSize } = useTheme();

  // Remembered on the machine so the next start does not flash the wrong
  // colours. Rust stores the name and does not know what it means (D21).
  const remember = (nextTheme: string, nextSize: string) => {
    if (!inApp()) return;
    call('set_appearance', { theme: nextTheme, textSize: nextSize }).catch(() => {
      /* A look that could not be remembered costs a restart, not a shop. */
    });
  };

  return (
    <div className="mb-appearance">
      <Card>
        <SectionHeader
          title="Theme"
          note="Changes as soon as you choose it. The sun and moon button in the title bar flips between light and dark."
        />
        <div className="mb-appearance__choices">
          {THEMES.map((option) => (
            <Radio
              key={option.id}
              name="mb-theme"
              label={option.name}
              checked={theme.id === option.id}
              onChange={() => {
                setTheme(option.id);
                remember(option.id, textSize);
              }}
            />
          ))}
        </div>
      </Card>

      <Card>
        <SectionHeader
          title="Text size"
          note="Scales the whole app, including the receipt preview. Audit F9: many owners are 50+ and the counter screen is across a desk."
        />
        <div className="mb-appearance__choices">
          {TEXT_SIZES.map((option) => (
            <Radio
              key={option.id}
              name="mb-text-size"
              label={option.name}
              checked={textSize === option.id}
              onChange={() => {
                setTextSize(option.id);
                remember(theme.id, option.id);
              }}
            />
          ))}
        </div>
      </Card>
    </div>
  );
}
