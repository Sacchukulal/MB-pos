/** How it looks. */

import { Radio, SectionHeader } from '../kit';
import { useTheme } from '../theme/ThemeProvider';
import { TEXT_SIZES } from '../theme/themes';

/*
 * Text size only. Light and dark are the sun and moon button in the title bar — one place to
 * change the theme, not two (owner, 2026-09-03).
 */
export function Appearance() {
  const { textSize, setTextSize } = useTheme();

  return (
    <div className="mb-appearance">
      <div className="mb-settings__topic">
        <SectionHeader
          title="Text size"
          note="Scales the whole app, including the receipt preview. Changes as soon as you choose it."
        />
        <div className="mb-appearance__choices">
          {TEXT_SIZES.map((option) => (
            <Radio
              key={option.id}
              name="mb-text-size"
              label={option.name}
              checked={textSize === option.id}
              onChange={() => setTextSize(option.id)}
            />
          ))}
        </div>
      </div>
    </div>
  );
}
