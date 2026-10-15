// Labels for the dashboard. Pure functions over the generated bindings; times are unix seconds.
import type { ActivationEvent } from "@/bindings/ActivationEvent";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { CheckoutStatus } from "@/bindings/CheckoutStatus";
import type { DownReason } from "@/bindings/DownReason";
import type { PromoteChannels } from "@/bindings/PromoteChannels";
import type { HostView } from "@/bindings/HostView";
import type { ServiceView } from "@/bindings/ServiceView";
import type { SkipReason } from "@/bindings/SkipReason";
import type { AgentStatus } from "@/lib/apiTypes";
import { ago } from "@/lib/time";

/** First 7 characters of a revision, keeping a `-dirty` suffix. */
export function shortRev(rev: string): string {
  const dirty = rev.endsWith("-dirty");
  const base = dirty ? rev.slice(0, -"-dirty".length) : rev;
  return base.slice(0, 7) + (dirty ? "-dirty" : "");
}

export function behindLabel(n: number | null): string {
  if (n === null) return "unknown";
  return n === 0 ? "up to date" : `${n} behind`;
}

export function onlineLabel(h: Pick<HostView, "online" | "last_seen">, now: number): string {
  switch (h.online) {
    case "on":
      return "Online";
    case "agent_unreachable":
      return "Agent not answering";
    case "off":
      return h.last_seen === null ? "Off" : `Off · last seen ${ago(h.last_seen, now)}`;
  }
}

/** Why an apply was skipped, as a phrase that follows "Skipped: ". */
export function skipReasonText(r: SkipReason, flakePath: string | null = null): string {
  const checkout = flakePath ?? "the checkout";
  switch (r) {
    case "fetch_failed":
      return "git fetch failed on the machine";
    case "commit_not_found":
      return "commit not found after fetching";
    case "commit_not_on_channel":
      return "commit is not on this machine's channel";
    case "wrong_branch":
      return `${checkout} is on a different branch`;
    case "dirty":
      return `local changes in ${checkout}`;
    case "not_fast_forward":
      return `${checkout} has commits that are not on the channel`;
  }
}

/** The state an apply is in, as the dashboard colours it (see the state system in the style guide). */
export type ApplyTone = "ok" | "running" | "skipped" | "failed" | "idle";

/** The text and tone of an apply's status badge. */
export function applyBadge(
  a: ApplyRecord | null,
  flakePath: string | null = null,
): { text: string; tone: ApplyTone } {
  if (a === null) return { text: "Never applied", tone: "idle" };
  switch (a.state) {
    case "idle":
      return { text: "Idle", tone: "idle" };
    case "running":
      return { text: "Applying", tone: "running" };
    case "ok":
      return { text: "Applied", tone: "ok" };
    case "skipped":
      return {
        text: a.skip_reason ? `Skipped: ${skipReasonText(a.skip_reason, flakePath)}` : "Skipped",
        tone: "skipped",
      };
    case "failed": {
      const what = a.message && /\bbuild\b/i.test(a.message) ? "Build failed" : "Failed";
      const code = a.exit_code !== null && a.exit_code !== 0 ? ` (exit ${a.exit_code})` : "";
      return { text: what + code, tone: "failed" };
    }
  }
}

/** The state of a machine as a whole: `warn` is an agent that does not answer. */
export type HostTone = ApplyTone | "off" | "warn";

/**
 * The one state that best describes a machine, used by both the summary tiles and the row
 * stripe. A running, failed or skipped apply wins (also on a machine that is off now), then an
 * agent that does not answer, then off; else ok when it runs its channel's newest commit.
 */
export function hostTone(h: HostView): HostTone {
  const t = applyBadge(h.last_apply).tone;
  if (t === "running" || t === "failed" || t === "skipped") return t;
  if (h.online === "agent_unreachable") return "warn";
  if (h.online === "off") return "off";
  return h.behind === 0 ? "ok" : "idle";
}

/** The one rule for "needs attention": a failed or skipped apply, or an agent not answering. */
export function needsAttention(t: HostTone): boolean {
  return t === "failed" || t === "skipped" || t === "warn";
}

/** A sentence for an agent's answer to an apply the server sent it, or null when it started. */
export function agentStatusText(host: string, s: AgentStatus): string | null {
  switch (s) {
    case "202":
      return null;
    case "200":
      return `${host} already ran this apply`;
    case "409":
      return `${host} is busy with another apply. This one is sent when it finishes.`;
    case "unreachable":
      return `${host}: agent not answering. The apply is queued and sent when it is back.`;
    case "error":
      return `${host}: the agent returned an error`;
  }
}

/** "1 machine", "3 machines". */
export function count(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

/** The four numbers at the top of the dashboard. */
export type FleetCounts = { total: number; upToDate: number; updating: number; attention: number };

/**
 * From `hostTone`: updating while an apply runs, needs attention by `needsAttention`, up to
 * date for the rest that run their channel's newest commit (on or off).
 */
export function fleetCounts(hosts: HostView[]): FleetCounts {
  let upToDate = 0;
  let updating = 0;
  let attention = 0;
  for (const h of hosts) {
    const t = hostTone(h);
    if (t === "running") updating++;
    else if (needsAttention(t)) attention++;
    else if (h.behind === 0) upToDate++;
  }
  return { total: hosts.length, upToDate, updating, attention };
}

/** A revision without its `-dirty` suffix. */
export const stripDirty = (rev: string) => rev.replace(/-dirty$/, "");

/** What is notable about a host's checkout; empty when it is clean and running. */
export function checkoutFlags(
  c: CheckoutStatus,
  running: string | null,
): { text: string; warn: boolean }[] {
  const flags: { text: string; warn: boolean }[] = [];
  if (c.dirty) flags.push({ text: "local changes", warn: true });
  if (c.untracked > 0) flags.push({ text: `${c.untracked} untracked`, warn: false });
  if (c.ahead !== null && c.ahead > 0) flags.push({ text: `${c.ahead} ahead`, warn: true });
  if (c.head !== null && running !== null && c.head !== stripDirty(running))
    flags.push({ text: "checkout is not the running system", warn: true });
  return flags;
}

/** Who or what switched the system, for an activation. */
export function triggerLabel(e: Pick<ActivationEvent, "trigger" | "sudo_user">): string {
  if (e.trigger.startsWith("apply:")) return "Lodestar apply";
  if (e.trigger === "manual") return e.sudo_user ? `manual (${e.sudo_user})` : "manual";
  return e.trigger;
}

/** What an audit row shows: which icon, a line of text, and a quieter line under it. */
export type AuditLine = {
  icon: "apply" | "applied" | "failed" | "skipped" | "activation" | "promote" | "other";
  title: string;
  meta: string;
};

const str = (v: unknown): string | null => (typeof v === "string" ? v : null);

export function auditLine(e: AuditEvent): AuditLine {
  const d = (e.details !== null && typeof e.details === "object" && !Array.isArray(e.details)
    ? e.details
    : {}) as Record<string, unknown>;
  const host = e.host ?? "?";
  switch (e.kind) {
    case "apply_requested": {
      const commit = str(d.commit);
      return {
        icon: "apply",
        title: `Apply newest on ${host}`,
        meta: commit ? `${e.actor} · ${shortRev(commit)}` : e.actor,
      };
    }
    case "apply_finished": {
      const rec = d as unknown as ApplyRecord;
      const b = applyBadge(typeof rec.state === "string" ? rec : null);
      const icon = b.tone === "ok" ? "applied" : b.tone === "failed" ? "failed" : b.tone === "skipped" ? "skipped" : "other";
      const commit = str(rec.commit);
      return { icon, title: `${host} · ${b.text}`, meta: commit ? `apply · ${shortRev(commit)}` : "apply" };
    }
    case "activation": {
      const trigger = str(d.trigger) ?? "";
      const rev = str(d.revision);
      if (trigger.startsWith("apply:"))
        return { icon: "applied", title: `${host} switched to ${rev ? shortRev(rev) : "a new system"}`, meta: "activation · apply" };
      const who = str(d.sudo_user);
      return {
        icon: "activation",
        title: trigger === "manual" ? `${host} switched by hand` : `${host} switched to ${rev ? shortRev(rev) : "a new system"}`,
        meta: ["activation", trigger || null, who].filter((x): x is string => !!x).join(" · "),
      };
    }
    case "promote": {
      const commit = str(d.commit);
      const channel = str(d.channel) ?? "channel";
      return { icon: "promote", title: `${channel} moved to ${commit ? shortRev(commit) : "?"}`, meta: `promote · ${e.actor}` };
    }
    default:
      return { icon: "other", title: e.host ? `${e.kind} on ${e.host}` : e.kind, meta: e.actor };
  }
}

/** How a channel is drawn: the promote target as `stable`, its source as `unstable`. */
export type ChannelRole = "stable" | "unstable" | "other";

export function channelRole(name: string, promote: PromoteChannels | null): ChannelRole {
  if (promote) return name === promote.to ? "stable" : name === promote.from ? "unstable" : "other";
  return name === "stable" ? "stable" : name === "unstable" ? "unstable" : "other";
}

/** Which brand mark an agent's `os` gets (NixOS for Linux hosts, which Lodestar runs on). */
export function osBrand(os: string): "nixos" | "apple" | null {
  const o = os.toLowerCase();
  if (o === "nixos" || o === "linux") return "nixos";
  if (o === "darwin" || o === "macos") return "apple";
  return null;
}

/** "NixOS", "macOS", or the agent's own word. */
export function osLabel(os: string): string {
  const b = osBrand(os);
  return b === "nixos" ? "NixOS" : b === "apple" ? "macOS" : os;
}

/** "2h ago · f63ae42 · Lodestar apply · gen 11": the last switch of a machine's system. */
export function activationText(e: ActivationEvent, now: number): string {
  return [ago(e.time, now), shortRev(e.revision), triggerLabel(e), e.generation !== null ? `gen ${e.generation}` : null]
    .filter((x): x is string => x !== null)
    .join(" · ");
}

/** Why a service check failed, as a phrase that follows "Down: ". */
export function downReasonLabel(r: DownReason): string {
  switch (r.kind) {
    case "http_status":
      return `HTTP ${r.detail}`;
    case "timeout":
      return "timed out";
    case "connection_refused":
      return "connection refused";
    case "unreachable":
      return "unreachable";
    case "tls":
      return "TLS error";
    case "other":
      return r.detail;
  }
}

/**
 * A service's status badge: "Up · 84 ms", "Down: HTTP 502", "Checking…", "Not checked". A
 * down service whose machine is off or whose agent does not answer names that as the cause.
 */
export function serviceLabel(s: ServiceView): string {
  const c = s.check;
  switch (c.state) {
    case "up":
      return c.latency_ms === null ? "Up" : `Up · ${c.latency_ms} ms`;
    case "down":
      if (s.host_online === "off") return "Down · host off";
      if (s.host_online === "agent_unreachable") return "Down · agent not answering";
      return c.reason === null ? "Down" : `Down: ${downReasonLabel(c.reason)}`;
    case "pending":
      return "Checking…";
    case "unchecked":
      return "Not checked";
  }
}

/**
 * "up since 3h" / "down since 2h": how long the service has been in its current state. The
 * server only pushes a change when the state or reason changes, so a "checked N ago" line
 * would read as a stalled checker. Under a minute it is "up just now"; null while pending
 * or unchecked.
 */
export function serviceSubLine(s: ServiceView, now: number): string | null {
  const c = s.check;
  if ((c.state !== "up" && c.state !== "down") || c.since === null) return null;
  const duration = ago(c.since, now);
  return duration === "just now" ? `${c.state} just now` : `${c.state} since ${duration.replace(/ ago$/, "")}`;
}

/** How many services are in each check state (the Services summary tiles). */
export function serviceCounts(list: ServiceView[]): { up: number; down: number; pending: number; unchecked: number } {
  const c = { up: 0, down: 0, pending: 0, unchecked: 0 };
  for (const s of list) c[s.check.state] += 1;
  return c;
}

/** Services by group, groups sorted by name and each group's cards by title. */
export function groupServices(list: ServiceView[]): [string, ServiceView[]][] {
  const groups = new Map<string, ServiceView[]>();
  for (const s of list) groups.set(s.group, [...(groups.get(s.group) ?? []), s]);
  return [...groups.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([g, xs]) => [g, [...xs].sort((a, b) => a.title.localeCompare(b.title))]);
}
