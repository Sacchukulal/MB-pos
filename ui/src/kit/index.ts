/** The UI kit — the only styling allowed in this product. */

import './layout.css';
import './kit.css';

/** The layout primitives. */
export {
  Fields,
  Foot,
  Notice,
  Page,
  PageHeader,
  Panel,
  Row,
  Scroller,
  Sections,
  SideFold,
  Stack,
  Toolbar,
} from './layout';

/** Where a new row's id comes from. */
export { freshId } from './ids';

/** The icon set. */
export { Icon } from './Icon';
export type { IconName, IconProps } from './Icon';

export {
  Button,
  Checkbox,
  Input,
  Keypad,
  /** The two fields that have a shape. */
  MoneyInput,
  NumberInput,
  PhoneInput,
  PHONE_DIGITS,
  onlyAmount,
  onlyPhone,
  Radio,
  SearchField,
  Select,
  Stepper,
} from './controls';
export type {
  ButtonProps,
  InputProps,
  MoneyInputProps,
  PhoneInputProps,
  SelectProps,
  StepperProps,
} from './controls';

export { useAction, type Action } from './action';
export { ConfirmDialog, Modal, ToastProvider, useReport, useToast } from './overlays';
export type { ToastTone } from './overlays';

export {
  Badge,
  Card,
  DateRangePicker,
  EmptyState,
  Locked,
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
export { InfoTip } from './InfoTip';

/** Join class names, dropping the falsy ones. */
export { cx } from './cx';

/** '3 items', never '3 item(s)'. */
export { plural } from './words';
