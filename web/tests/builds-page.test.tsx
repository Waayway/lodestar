// @vitest-environment jsdom
// The Builds tab: every recent build with its hosts, logs and Retry build.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it } from "vitest";
import type { BuildView } from "@/bindings/BuildView";
import type { BuildsView } from "@/bindings/BuildsView";
import { NavTabs } from "@/components/layout/NavTabs";
import { parseRoute } from "@/lib/route";
import { BuildsList } from "@/pages/BuildsPage";
import { fakeActions, me, NOW, REV, withActions } from "./helpers";

const OTHER = "fedcba9876543210fedcba9876543210fedcba98";
const PATH = "/nix/store/0123456789abcdfghijklmnpqrsvwxyz-nixos-system-router";

const build = (over: Partial<BuildView>): BuildView => ({
  commit: REV,
  subject: "feat(cache): turn on the gate",
  building: [],
  head_of: ["stable"],
  cached_for: [],
  hosts: [{ name: "router", ok: true, path: PATH, log: null, built_at: NOW - 120 }],
  finished_at: NOW - 120,
  ...over,
});
const view = (builds: BuildView[], enabled = true): BuildsView => ({ enabled, builds });
const admin = { viewer: me({}) };
const reader = { viewer: me({ viewer: "alice", role: "reader", can_act: false, reason: "not_admin" }) };

function show(v: BuildsView, viewer = admin, actions = fakeActions()) {
  const r = render(withActions(actions, <BuildsList view={v} now={NOW} />, viewer));
  return { ...r, actions };
}

afterEach(cleanup);

describe("the Builds tab", () => {
  it("is reached at #/builds", () => {
    expect(parseRoute("#/builds")).toBe("builds");
    expect(parseRoute("#/services")).toBe("services");
    expect(parseRoute("")).toBe("fleet");
  });

  it("shows a tab with the number of failed channel builds", () => {
    render(<NavTabs route="builds" downCount={0} failedBuilds={2} />);
    const tab = screen.getByRole("link", { name: /Builds/ });
    expect(tab.getAttribute("href")).toBe("#/builds");
    expect(tab.getAttribute("aria-current")).toBe("page");
    expect(tab.textContent).toContain("2 failed");
  });

  it("says the gate is off when it is and nothing was built", () => {
    show(view([], false));
    expect(screen.getByText("The build gate is off")).toBeTruthy();
  });

  it("says no builds yet with the gate on", () => {
    show(view([]));
    expect(screen.getByText("No builds yet")).toBeTruthy();
  });

  it("lists a good build with its subject, channels, age and store paths", () => {
    const { container } = show(view([build({ cached_for: ["stable"] })]));
    const card = container.querySelector(`[data-build-commit="${REV}"]`);
    expect(card?.getAttribute("data-build")).toBe("ok");
    expect(card?.textContent).toContain("feat(cache): turn on the gate");
    expect(card?.textContent).toContain("Built 1 machine");
    expect(card?.textContent).toContain("head of stable");
    expect(card?.textContent).toContain("in the cache for stable");
    expect(card?.textContent).toContain("2m ago");
    expect(card?.querySelector("[data-host=router]")?.textContent).toContain(PATH);
    expect(screen.queryByRole("button", { name: "Retry build" })).toBeNull();
  });

  it("shows a failed host's log and retries the commit for an admin", () => {
    const failed = build({
      hosts: [
        { name: "monitor", ok: false, path: null, log: "error: builder for 'x.drv' failed", built_at: NOW - 60 },
        { name: "router", ok: true, path: PATH, log: null, built_at: NOW - 60 },
      ],
    });
    const { container, actions } = show(view([failed]));
    const card = container.querySelector(`[data-build-commit="${REV}"]`);
    expect(card?.getAttribute("data-build")).toBe("failed");
    expect(card?.textContent).toContain("1 of 2 machines failed");
    const row = card?.querySelector("[data-host=monitor]");
    expect(row?.textContent).toContain("Failed:");
    expect(row?.querySelector("summary")?.textContent).toBe("Show log");
    expect(row?.querySelector("pre")?.textContent).toBe("error: builder for 'x.drv' failed");
    fireEvent.click(screen.getByRole("button", { name: "Retry build" }));
    expect(actions.retryBuild).toHaveBeenCalledWith(REV);
  });

  it("hides Retry build from readers", () => {
    show(view([build({ hosts: [{ name: "monitor", ok: false, path: null, log: "no", built_at: NOW }] })]), reader);
    expect(screen.queryByRole("button", { name: "Retry build" })).toBeNull();
  });

  it("shows a commit that is building, before its finished ones", () => {
    const { container } = show(
      view([build({ commit: OTHER, building: ["stable"], hosts: [], finished_at: null }), build({ head_of: [] })]),
    );
    const cards = [...container.querySelectorAll("[data-build-commit]")];
    expect(cards.map((c) => c.getAttribute("data-build"))).toEqual(["building", "ok"]);
    expect(cards[0].textContent).toContain("Building for stable…");
    expect(cards[0].querySelector("svg")?.classList.contains("animate-spin")).toBe(true);
  });
});
