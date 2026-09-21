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
  Rail,
  RailItem,
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

/** The brand mark. */
export { Logo } from './Logo';
export type { LogoProps } from './Logo';

export {
  Button,
  Checkbox,
  Choice,
  Input,
  Keypad,
  /** The two fields that have a shape. */
  MoneyInput,
  NumberInput,
  PhoneInput,
  PHONE_DIGITS,
  onlyAmount,
  onlyPhone,
  Pick,
  Radio,
  SearchField,
  Segment,
  Select,
  Stepper,
  Switch,
} from './controls';
export type {
  ButtonProps,
  FieldSize,
  PickProps,
  InputProps,
  MoneyInputProps,
  PhoneInputProps,
  SelectProps,
  StepperProps,
  SwitchProps,
} from './controls';

export { useAction, type Action } from './action';
export { ConfirmDialog, Modal, RowMenu, ToastProvider, useReport, useToast } from './overlays';
export type { ToastTone } from './overlays';

export {
  Badge,
  Card,
  DateRangePicker,
  EmptyState,
  Fact,
  Facts,
  Hint,
  Locked,
  Money,
  Numeric,
  SaveBar,
  SectionHeader,
  Spinner,
  StatCard,
  Stats,
  Caption,
  Table,
  Tabs,
} from './display';
export type { BadgeTone, Column } from './display';
export { InfoTip } from './InfoTip';
export { Chart } from './charts';

/** Join class names, dropping the falsy ones. */
export { cx } from './cx';

/** '3 items', never '3 item(s)'. */
export { plural } from './words';
export { beatsFor, useDevicePixelRatio } from './screen';
