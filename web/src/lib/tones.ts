// The one place that turns a state into colour classes and an icon. Components look their
// state up here instead of keeping their own maps; docs/style-guide.md ("State system")
// describes the same table.
import type { OnlineState } from "@/bindings/OnlineState";
import type { ServiceView } from "@/bindings/ServiceView";
import {
  Activation,
  Applied,
  Apply,
  Audit,
  Building,
  Disconnected,
  Down,
  Failed,
  Forbidden,
  type Icon,
  Idle,
  Off,
  Promote,
  Skipped,
  Unreachable,
  Up,
} from "@/icons";
import type { ApplyTone, AuditLine, HostTone } from "@/lib/format";

/** The colour families of the tokens. `neutral` is muted text on a chip. */
export type Tone = "ok" | "info" | "warn" | "bad" | "accent" | "neutral";

/** Text (and icon) colour of a tone. */
export const toneText: Record<Tone, string> = {
  ok: "text-ok",
  info: "text-info",
  warn: "text-warn",
  bad: "text-bad",
  accent: "text-accent",
  neutral: "text-muted",
};

/** The soft wash behind a tone's text (badges, tile icons). */
export const toneSoft: Record<Tone, string> = {
  ok: "bg-ok-soft",
  info: "bg-info-soft",
  warn: "bg-warn-soft",
  bad: "bg-bad-soft",
  accent: "bg-accent-soft",
  neutral: "bg-chip",
};

/** Solid fill (dots). */
export const toneFill: Record<Tone, string> = {
  ok: "bg-ok",
  info: "bg-info",
  warn: "bg-warn",
  bad: "bg-bad",
  accent: "bg-accent",
  neutral: "bg-muted",
};

/** A 3 px state stripe on the left edge of a table row's first cell. */
export const toneStripe: Record<Tone, string> = {
  ok: "shadow-[inset_3px_0_0_var(--ok)]",
  info: "shadow-[inset_3px_0_0_var(--info)]",
  warn: "shadow-[inset_3px_0_0_var(--warn)]",
  bad: "shadow-[inset_3px_0_0_var(--bad)]",
  accent: "shadow-[inset_3px_0_0_var(--accent)]",
  neutral: "shadow-[inset_3px_0_0_var(--muted)]",
};

/** Text on its soft wash: a badge or an icon square. */
export const toneChip = (t: Tone) => `${toneSoft[t]} ${toneText[t]}`;

export type StateStyle = { tone: Tone; icon: Icon | null; spin?: boolean };

/** An apply's state (StatusBadge, audit rows). */
export const applyStyle: Record<ApplyTone, StateStyle & { icon: Icon }> = {
  ok: { tone: "ok", icon: Applied },
  running: { tone: "info", icon: Building, spin: true },
  skipped: { tone: "warn", icon: Skipped },
  failed: { tone: "bad", icon: Failed },
  idle: { tone: "neutral", icon: Idle },
};

/** Whether a machine is on and its agent answers (OnlineState). On is a dot, not an icon. */
export const onlineStyle: Record<OnlineState, StateStyle> = {
  on: { tone: "ok", icon: null },
  agent_unreachable: { tone: "warn", icon: Unreachable },
  off: { tone: "neutral", icon: Off },
};

/** A machine as a whole (the row stripe); idle machines get none. */
export const hostStyle: Record<HostTone, Tone | null> = {
  ok: "ok",
  running: "info",
  skipped: "warn",
  failed: "bad",
  warn: "warn",
  off: "neutral",
  idle: null,
};

/** The WebSocket feed's state (LivePill). Live and connecting are a dot. */
export const feedStyle: Record<"connecting" | "live" | "waiting" | "forbidden", StateStyle> = {
  live: { tone: "ok", icon: null },
  connecting: { tone: "info", icon: null },
  waiting: { tone: "bad", icon: Disconnected },
  forbidden: { tone: "bad", icon: Forbidden },
};

/** How far behind its channel a machine is (BehindCount). */
export function behindTone(behind: number | null): Tone {
  return behind === null ? "neutral" : behind === 0 ? "ok" : "warn";
}

/** Audit rows: every icon in the accent, on a chip. */
export const auditIcon: Record<AuditLine["icon"], Icon> = {
  apply: Apply,
  applied: Applied,
  failed: Failed,
  skipped: Skipped,
  activation: Activation,
  promote: Promote,
  other: Audit,
};

/** A check slower than this is up but drawn in `warn`. */
export const SLOW_MS = 1000;

/**
 * A service's state (ServiceCard). A down service on a machine that is off or whose agent does
 * not answer is `warn` with the machine's icon: the machine is the cause, not the service.
 */
export function serviceStyle(s: ServiceView): StateStyle & { icon: Icon } {
  const c = s.check;
  switch (c.state) {
    case "up":
      return { tone: c.latency_ms !== null && c.latency_ms >= SLOW_MS ? "warn" : "ok", icon: Up };
    case "down":
      if (s.host_online === "off") return { tone: "warn", icon: Off };
      if (s.host_online === "agent_unreachable") return { tone: "warn", icon: Unreachable };
      return { tone: "bad", icon: Down };
    case "pending":
      return { tone: "info", icon: Building, spin: true };
    case "unchecked":
      return { tone: "neutral", icon: Idle };
  }
}
