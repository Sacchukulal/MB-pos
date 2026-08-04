/**
 * The UI kit — **the only styling allowed in this product.**
 *
 * A screen imports from here and composes. It does not write CSS, it does not
 * write a colour, and it does not invent a confirmation dialog. Audit E11 is
 * what happens otherwise: *"208 hand-written inline styles have crept back
 * across 18 screen files, fighting the design-token system that was built to
 * prevent exactly that."*
 *
 * Grouped by family rather than one file per component, deliberately: at
 * twenty-one components, one file each is twenty-one imports and a barrel that
 * has to be kept in step. Split a family when it stops fitting on a screen —
 * splitting a file is free.
 */

import './kit.css';

export { Button, Checkbox, Input, Keypad, NumberInput, Radio, SearchField, Select } from './controls';
export type { ButtonProps, InputProps, SelectProps } from './controls';

export { ConfirmDialog, Modal, ToastProvider, useToast } from './overlays';
export type { ToastTone } from './overlays';

export {
  Badge,
  Card,
  DateRangePicker,
  EmptyState,
  Money,
  Numeric,
  SaveBar,
  SectionHeader,
  Spinner,
  StatCard,
  Table,
  Tabs,
} from './display';
export type { BadgeTone, Column } from './display';
