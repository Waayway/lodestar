// The Lucide icons the dashboard uses, under Lodestar names. Components import from here,
// never from lucide-react directly, so the set stays small and named by meaning.
// docs/style-guide.md lists them.
export {
  PlayIcon as Apply,
  ChevronsUpIcon as Promote,
  ScrollTextIcon as Log,
  CheckIcon as Applied,
  RotateCwIcon as Building,
  PenLineIcon as Skipped,
  CircleXIcon as Failed,
  CircleDashedIcon as Idle,
  PowerIcon as Off,
  WifiOffIcon as Unreachable,
  TriangleAlertIcon as Attention,
  MonitorIcon as Machines,
  GitBranchIcon as Channel,
  TerminalIcon as Activation,
  HistoryIcon as Audit,
  UserRoundIcon as Viewer,
  ClockIcon as Queued,
  XIcon as Close,
  SunIcon as ThemeLight,
  MoonIcon as ThemeDark,
  SunMoonIcon as ThemeSystem,
  CircleCheckIcon as ToastSuccess,
  InfoIcon as ToastInfo,
  TriangleAlertIcon as ToastWarning,
  OctagonXIcon as ToastError,
  ShieldOffIcon as Forbidden,
  CloudOffIcon as Disconnected,
} from "lucide-react";

/** The type of any icon above (for props that take an icon). */
export type { LucideIcon as Icon } from "lucide-react";
