// @vitest-environment jsdom
// The build of a channel's head on the builder, as the channel card shows it.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it } from "vitest";
import type { ChannelBuildView } from "@/bindings/ChannelBuildView";
import type { ChannelView } from "@/bindings/ChannelView";
import { ChannelCard } from "@/components/fleet/ChannelCard";
import { fakeActions, host, me, REV, withActions } from "./helpers";

const channel = (build: ChannelBuildView | null): ChannelView => ({
  name: "stable",
  head: REV,
  head_subject: "fix(users): linger",
  build,
});
const state = (over: Partial<ChannelBuildView>): ChannelBuildView => ({
  commit: REV,
  state: "ok",
  failed_hosts: [],
  finished_at: 1n,
  ...over,
});
const admin = { viewer: me({}) };
const reader = { viewer: me({ viewer: "alice", role: "reader", can_act: false, reason: "not_admin" }) };

function show(build: ChannelBuildView | null, viewer = admin, actions = fakeActions()) {
  const r = render(
    withActions(actions, <ChannelCard channel={channel(build)} hosts={[host({})]} role="stable" />, viewer),
  );
  return { ...r, actions };
}

afterEach(cleanup);

describe("build state on the channel card", () => {
  it("shows nothing without a build", () => {
    const { container } = show(null);
    expect(container.querySelector("[data-build]")).toBeNull();
    expect(screen.queryByRole("button", { name: "Retry build" })).toBeNull();
  });

  it("says Building on the builder… with an icon, and no retry", () => {
    const { container } = show(state({ state: "building", finished_at: null }));
    const line = container.querySelector("[data-build=building]");
    expect(line?.textContent).toContain("Building on the builder…");
    expect(line?.querySelector("svg")).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Retry build" })).toBeNull();
  });

  it("says Built with an icon", () => {
    const { container } = show(state({}));
    const line = container.querySelector("[data-build=ok]");
    expect(line?.textContent).toContain("Built");
    expect(line?.querySelector("svg")).toBeTruthy();
  });

  it("names the failed hosts and retries the commit for an admin", () => {
    const { container, actions } = show(state({ state: "failed", failed_hosts: ["manager", "prod"] }));
    const line = container.querySelector("[data-build=failed]");
    expect(line?.textContent).toContain("Build failed: manager, prod");
    expect(line?.querySelector("svg")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Retry build" }));
    expect(actions.retryBuild).toHaveBeenCalledWith(REV);
  });

  it("hides the retry button from readers but still shows the failure", () => {
    const { container } = show(state({ state: "failed", failed_hosts: ["prod"] }), reader);
    expect(container.textContent).toContain("Build failed: prod");
    expect(screen.queryByRole("button", { name: "Retry build" })).toBeNull();
  });
});
