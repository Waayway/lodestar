// Labels and copy: everything the dashboard says about a host, an apply or the fleet.
import { describe, expect, it } from "vitest";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { HostView } from "@/bindings/HostView";
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { CheckoutStatus } from "@/bindings/CheckoutStatus";
import {
  agentStatusText,
  applyBadge,
  auditLine,
  behindLabel,
  channelRole,
  checkoutFlags,
  fleetCounts,
  onlineLabel,
  osBrand,
  osLabel,
  shortRev,
  skipReasonText,
  triggerLabel,
} from "@/lib/format";
import { ago, durationLabel } from "@/lib/time";

const REV = "0123456789abcdef0123456789abcdef01234567";

function host(over: Partial<HostView>): HostView {
  return {
    name: "desktop",
    os: "nixos",
    channel: "unstable",
    online: "on",
    last_seen: null,
    running: REV,
    running_dirty: false,
    behind: 0,
    checkout: null,
    last_apply: null,
    desired_apply_id: null,
    last_activation: null,
    ...over,
  };
}

function apply(over: Partial<ApplyRecord>): ApplyRecord {
  return {
    apply_id: "a1",
    commit: REV,
    state: "ok",
    skip_reason: null,
    message: null,
    exit_code: null,
    started_at: 100,
    finished_at: 110,
    ...over,
  };
}

describe("shortRev", () => {
  it("keeps 7 characters", () => {
    expect(shortRev(REV)).toBe("0123456");
  });
  it("keeps -dirty", () => {
    expect(shortRev(`${REV}-dirty`)).toBe("0123456-dirty");
  });
  it("leaves short strings alone", () => {
    expect(shortRev("unknown")).toBe("unknown");
  });
});

describe("behindLabel", () => {
  it("up to date", () => expect(behindLabel(0)).toBe("up to date"));
  it("n behind", () => expect(behindLabel(3)).toBe("3 behind"));
  it("unknown", () => expect(behindLabel(null)).toBe("unknown"));
});

describe("onlineLabel", () => {
  const now = 10_000;
  it("off with last seen", () => {
    expect(onlineLabel(host({ online: "off", last_seen: now - 5 * 60 }), now)).toBe(
      "Off · last seen 5m ago",
    );
  });
  it("off, last seen hours ago", () => {
    expect(onlineLabel(host({ online: "off", last_seen: now - 2 * 3600 - 30 }), now)).toBe(
      "Off · last seen 2h ago",
    );
  });
  it("off, never seen", () => {
    expect(onlineLabel(host({ online: "off", last_seen: null }), now)).toBe("Off");
  });
  it("agent unreachable", () => {
    expect(onlineLabel(host({ online: "agent_unreachable" }), now)).toBe("Agent not answering");
  });
  it("on", () => {
    expect(onlineLabel(host({ online: "on" }), now)).toBe("Online");
  });
});

describe("applyBadge", () => {
  it("no apply yet", () => {
    expect(applyBadge(null)).toEqual({ text: "Never applied", tone: "idle" });
  });
  it("running", () => {
    expect(applyBadge(apply({ state: "running", finished_at: null }))).toEqual({
      text: "Applying",
      tone: "running",
    });
  });
  it("ok", () => {
    expect(applyBadge(apply({ state: "ok", exit_code: 0 }))).toEqual({
      text: "Applied",
      tone: "ok",
    });
  });
  it("skipped on a dirty checkout", () => {
    expect(applyBadge(apply({ state: "skipped", skip_reason: "dirty" }))).toEqual({
      text: "Skipped: local changes in the checkout",
      tone: "skipped",
    });
    expect(
      applyBadge(apply({ state: "skipped", skip_reason: "dirty" }), "/home/alice/infra"),
    ).toEqual({ text: "Skipped: local changes in /home/alice/infra", tone: "skipped" });
  });
  it("failed build with exit code", () => {
    expect(
      applyBadge(
        apply({ state: "failed", exit_code: 1, message: "nix build failed (exit status: 1)" }),
      ),
    ).toEqual({ text: "Build failed (exit 1)", tone: "failed" });
  });
  it("failed without exit code", () => {
    expect(applyBadge(apply({ state: "failed", message: "interrupted" }))).toEqual({
      text: "Failed",
      tone: "failed",
    });
  });
  it("idle", () => {
    expect(applyBadge(apply({ state: "idle" }))).toEqual({ text: "Idle", tone: "idle" });
  });
});

describe("skipReasonText", () => {
  it("has a distinct phrase for every reason", () => {
    const reasons = [
      "fetch_failed",
      "commit_not_found",
      "commit_not_on_channel",
      "wrong_branch",
      "dirty",
      "not_fast_forward",
    ] as const;
    const texts = reasons.map((r) => skipReasonText(r));
    for (const t of texts) expect(t).toMatch(/^[a-z].{8,}$/);
    expect(new Set(texts).size).toBe(reasons.length);
    expect(skipReasonText("dirty")).toBe("local changes in the checkout");
    expect(skipReasonText("wrong_branch", "/Users/alice/.flake")).toBe(
      "/Users/alice/.flake is on a different branch",
    );
  });
});

describe("time labels", () => {
  it("ago", () => {
    expect(ago(100, 130)).toBe("just now");
    expect(ago(0, 5 * 60)).toBe("5m ago");
    expect(ago(0, 3 * 86400)).toBe("3d ago");
    expect(ago(200, 100)).toBe("just now");
  });
  it("durationLabel", () => {
    expect(durationLabel(null, null, 100)).toBe("");
    expect(durationLabel(100, 145, 999)).toBe("45s");
    expect(durationLabel(100, null, 225)).toBe("2m 05s");
  });
});

describe("agentStatusText", () => {
  it("is silent when the apply started, and explains every other answer", () => {
    expect(agentStatusText("desktop", "202")).toBeNull();
    expect(agentStatusText("desktop", "200")).toBe("desktop already ran this apply");
    expect(agentStatusText("desktop", "unreachable")).toBe(
      "desktop: agent not answering. The apply is queued and sent when it is back.",
    );
    expect(agentStatusText("desktop", "409")).toMatch(/^desktop is busy/);
    expect(agentStatusText("desktop", "error")).toBe("desktop: the agent returned an error");
  });
});

describe("fleetCounts", () => {
  it("splits the fleet into up to date, updating and needing attention", () => {
    const hosts = [
      host({ name: "a", behind: 0, last_apply: apply({ state: "ok" }) }),
      host({ name: "b", behind: 2, last_apply: apply({ state: "running", finished_at: null }) }),
      host({ name: "c", behind: 1, last_apply: apply({ state: "failed" }) }),
      host({ name: "d", online: "off", behind: 3, last_apply: apply({ state: "skipped", skip_reason: "dirty" }) }),
      host({ name: "e", online: "agent_unreachable", behind: 0 }),
      host({ name: "f", behind: 4 }),
    ];
    expect(fleetCounts(hosts)).toEqual({ total: 6, upToDate: 1, updating: 1, attention: 3 });
    expect(fleetCounts([])).toEqual({ total: 0, upToDate: 0, updating: 0, attention: 0 });
  });
});

describe("checkoutFlags", () => {
  const co = (over: Partial<CheckoutStatus>): CheckoutStatus => ({
    branch: "main",
    head: REV,
    dirty: false,
    untracked: 0,
    ahead: 0,
    last_fetch_at: null,
    flake_path: null,
    ...over,
  });
  it("is empty for a clean checkout of the running system", () => {
    expect(checkoutFlags(co({}), REV)).toEqual([]);
    expect(checkoutFlags(co({}), `${REV}-dirty`)).toEqual([]);
  });
  it("lists local changes, untracked files, commits ahead and a different checkout", () => {
    expect(checkoutFlags(co({ dirty: true, untracked: 2, ahead: 1, head: "f".repeat(40) }), REV)).toEqual([
      { text: "local changes", warn: true },
      { text: "2 untracked", warn: false },
      { text: "1 ahead", warn: true },
      { text: "checkout is not the running system", warn: true },
    ]);
  });
});

describe("triggerLabel", () => {
  it("names Lodestar applies and manual switches", () => {
    expect(triggerLabel({ trigger: "apply:a1", sudo_user: null })).toBe("Lodestar apply");
    expect(triggerLabel({ trigger: "manual", sudo_user: "alice" })).toBe("manual (alice)");
    expect(triggerLabel({ trigger: "manual", sudo_user: null })).toBe("manual");
    expect(triggerLabel({ trigger: "boot", sudo_user: null })).toBe("boot");
  });
});

describe("channelRole and OS", () => {
  it("draws the promote target as stable and its source as unstable", () => {
    const pair = { from: "main", to: "release" };
    expect(channelRole("release", pair)).toBe("stable");
    expect(channelRole("main", pair)).toBe("unstable");
    expect(channelRole("other", pair)).toBe("other");
    expect(channelRole("stable", null)).toBe("stable");
    expect(channelRole("main", null)).toBe("other");
  });
  it("maps agent OS words to brands and labels", () => {
    expect(osBrand("nixos")).toBe("nixos");
    expect(osBrand("linux")).toBe("nixos");
    expect(osBrand("darwin")).toBe("apple");
    expect(osBrand("freebsd")).toBeNull();
    expect(osLabel("darwin")).toBe("macOS");
    expect(osLabel("linux")).toBe("NixOS");
    expect(osLabel("freebsd")).toBe("freebsd");
  });
});

describe("auditLine", () => {
  const ev = (over: Partial<AuditEvent>): AuditEvent => ({
    id: 1,
    time: 0,
    host: "desktop",
    kind: "apply_requested",
    actor: "alice@example.com",
    details: {},
    ...over,
  });
  it("apply requested", () => {
    expect(auditLine(ev({ details: { commit: REV, apply_id: "a1", channel: "unstable" } }))).toEqual({
      icon: "apply",
      title: "Apply newest on desktop",
      meta: "alice@example.com · 0123456",
    });
  });
  it("apply finished, in the badge's words", () => {
    const rec = apply({ state: "failed", exit_code: 1, message: "nix build failed" });
    expect(auditLine(ev({ kind: "apply_finished", actor: "agent", details: rec as never }))).toEqual({
      icon: "failed",
      title: "desktop · Build failed (exit 1)",
      meta: "apply · 0123456",
    });
  });
  it("activations by an apply and by hand", () => {
    expect(
      auditLine(ev({ kind: "activation", details: { trigger: "apply:a1", revision: REV, sudo_user: null } })),
    ).toEqual({ icon: "applied", title: "desktop switched to 0123456", meta: "activation · apply" });
    expect(
      auditLine(ev({ kind: "activation", details: { trigger: "manual", revision: REV, sudo_user: "alice" } })),
    ).toEqual({ icon: "activation", title: "desktop switched by hand", meta: "activation · manual · alice" });
  });
  it("promote, and kinds it doesn't know", () => {
    expect(
      auditLine(ev({ kind: "promote", host: null, details: { commit: REV, previous: null, channel: "stable" } })),
    ).toEqual({ icon: "promote", title: "stable moved to 0123456", meta: "promote · alice@example.com" });
    expect(auditLine(ev({ kind: "something_new", details: null }))).toEqual({
      icon: "other",
      title: "something_new on desktop",
      meta: "alice@example.com",
    });
  });
});
