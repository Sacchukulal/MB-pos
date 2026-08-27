/** How it looks. */

import { Card, Radio, SectionHeader } from '../kit';
import { call, inApp } from '../ipc/call';
import { useTheme } from '../theme/ThemeProvider';
import { TEXT_SIZES, THEMES } from '../theme/themes';

export function Appearance() {
  const { theme, textSize, setTheme, setTextSize } = useTheme();

  // Remembered on the machine so the next start does not flash the wrong colours.
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
