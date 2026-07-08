export type ScreenId =
  | "dashboard"
  | "billing"
  | "expenses"
  | "reports"
  | "account"
  | "settings-general"
  | "settings-bill"
  | "settings-printer"
  | "settings-menu"
  | "settings-staff";

export const SETTINGS_SCREENS: ScreenId[] = [
  "settings-general",
  "settings-bill",
  "settings-printer",
  "settings-menu",
  "settings-staff",
];
