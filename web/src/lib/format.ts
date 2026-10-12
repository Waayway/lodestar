// Labels for the dashboard. Pure functions over the generated bindings; times are unix seconds.
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { HostView } from "@/bindings/HostView";
import type { SkipReason } from "@/bindings/SkipReason";
import type { AgentStatus } from "@/connection/client";
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

export function onlineLabel(h: HostView, now: number): string {
  switch (h.online) {
    case "on":
      return "on";
    case "agent_unreachable":
      return "on, agent unreachable";
    case "off":
      return h.last_seen === null ? "off" : `off · last seen ${ago(h.last_seen, now)}`;
  }
}

/** Why an apply was skipped, as a phrase that follows "Skipped: ". */
export function skipReasonText(r: SkipReason, flakePath: string | null = null): string {
  const checkout = flakePath ?? "the checkout";
  switch (r) {
    case "fetch_failed":
      return "git fetch failed on the host";
    case "commit_not_found":
      return "commit not found after fetching";
    case "commit_not_on_channel":
      return "commit is not on this host's channel";
    case "wrong_branch":
      return `${checkout} is on a different branch`;
    case "dirty":
      return `local changes in ${checkout}`;
    case "not_fast_forward":
      return `${checkout} has commits that are not on the channel`;
  }
}

export type BadgeVariant = "default" | "secondary" | "destructive" | "outline";

export function applyBadge(
  a: ApplyRecord | null,
  flakePath: string | null = null,
): { text: string; variant: BadgeVariant } {
  if (a === null) return { text: "Never applied", variant: "outline" };
  switch (a.state) {
    case "idle":
      return { text: "Idle", variant: "outline" };
    case "running":
      return { text: "Applying", variant: "secondary" };
    case "ok":
      return { text: "Applied", variant: "default" };
    case "skipped":
      return {
        text: a.skip_reason ? `Skipped: ${skipReasonText(a.skip_reason, flakePath)}` : "Skipped",
        variant: "outline",
      };
    case "failed": {
      const what = a.message && /\bbuild\b/i.test(a.message) ? "Build failed" : "Failed";
      const code = a.exit_code !== null && a.exit_code !== 0 ? ` (exit ${a.exit_code})` : "";
      return { text: what + code, variant: "destructive" };
    }
  }
}

/** Semantic tone of a host row: drives the status stripe and badge colours. */
export type Tone = "ok" | "running" | "skipped" | "failed" | "off" | "warn" | "idle";

export function applyTone(a: ApplyRecord | null): Tone {
  if (a === null) return "idle";
  switch (a.state) {
    case "ok":
      return "ok";
    case "running":
      return "running";
    case "skipped":
      return "skipped";
    case "failed":
      return "failed";
    case "idle":
      return "idle";
  }
}

/** The one tone that best describes the host: an off machine or a broken apply wins. */
export function hostTone(h: HostView): Tone {
  if (h.online === "off") return "off";
  const t = applyTone(h.last_apply);
  if (t === "running" || t === "failed" || t === "skipped") return t;
  if (h.online === "agent_unreachable") return "warn";
  return h.behind === 0 ? "ok" : "idle";
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
