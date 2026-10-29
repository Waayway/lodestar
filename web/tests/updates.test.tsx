// @vitest-environment jsdom
// Software versions in the fleet table and the updates panel.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it } from "vitest";
import type { SoftwareView } from "@/bindings/SoftwareView";
import type { UpdatesView } from "@/bindings/UpdatesView";
import { UpdatesCard } from "@/components/fleet/UpdatesCard";
import { VersionsCell } from "@/components/fleet/VersionsCell";
import { compareVersions, isBehind } from "@/lib/versions";
import { fakeActions, me, NOW, REV, withActions } from "./helpers";

afterEach(cleanup);

const sw = (over: Partial<SoftwareView> = {}): SoftwareView => ({
  name: "tailscale",
  input: "nixpkgs",
  latest: "1.104.2",
  checked_at: NOW - 120,
  error: null,
  ...over,
});

const updates = (over: Partial<UpdatesView> = {}): UpdatesView => ({
  software: [sw()],
  inputs: [
    {
      name: "nixpkgs",
      last: {
        finished_at: NOW - 3600,
        result: "pushed",
        commit: REV,
        changes: [{ software: "tailscale", from: "1.98.10", to: "1.104.2" }],
        host: null,
      },
    },
  ],
  running: false,
  ...over,
});

describe("compareVersions", () => {
  it("compares dot parts numerically, strips v and suffixes, missing is zero", () => {
    expect(compareVersions("1.98.10", "1.104.2")).toBe(-1);
    expect(compareVersions("v1.104.2", "1.104.2")).toBe(0);
    expect(compareVersions("1.104.2-rc1", "1.104.2+build")).toBe(0);
    expect(compareVersions("1.2", "1.2.0")).toBe(0);
    expect(compareVersions("2.0", "1.9.9")).toBe(1);
  });
  it("is unknown for non-numeric versions", () => {
    expect(compareVersions("unstable", "1.0")).toBeNull();
    expect(compareVersions("1.x", "1.0")).toBeNull();
    expect(isBehind("unstable", "1.0")).toBe(false);
    expect(isBehind("1.0", null)).toBe(false);
    expect(isBehind("1.0", "1.1")).toBe(true);
  });
});

describe("VersionsCell", () => {
  it("marks a version behind the latest release", () => {
    render(<VersionsCell versions={{ tailscale: "1.98.10" }} software={[sw()]} />);
    const el = screen.getByText("1.98.10");
    expect(el.closest("[data-behind]")?.getAttribute("data-behind")).toBe("true");
    expect(el.className).toContain("text-warn");
    expect(screen.getByText("tailscale")).toBeTruthy();
    // Not colour alone: visible latest version and accessible text.
    expect(screen.getByText("Behind, latest")).toBeTruthy();
    expect(screen.getByText("1.104.2")).toBeTruthy();
    expect(el.closest("li")?.textContent).toContain("→ 1.104.2");
  });
  it("is plain when equal", () => {
    render(<VersionsCell versions={{ tailscale: "1.104.2" }} software={[sw()]} />);
    const el = screen.getByText("1.104.2");
    expect(el.closest("[data-behind]")?.getAttribute("data-behind")).toBe("false");
    expect(el.className).not.toContain("text-warn");
    expect(screen.queryByText(/Behind/)).toBeNull();
  });
  it("shows nothing when the host reports no versions", () => {
    const { container } = render(<VersionsCell versions={{}} software={[sw()]} />);
    expect(container.textContent).toBe("");
  });
});

describe("UpdatesCard", () => {
  const ui = (u: UpdatesView, viewer = me({}), actions = fakeActions()) =>
    render(withActions(actions, <UpdatesCard updates={u} now={NOW} />, { viewer, updates: u }));

  it("shows the last result and its age", () => {
    ui(updates());
    expect(screen.getByText("nixpkgs")).toBeTruthy();
    expect(screen.getByText("Pushed")).toBeTruthy();
    expect(screen.getByText(/1h ago/)).toBeTruthy();
    expect(screen.getByText("0123456")).toBeTruthy();
    expect(screen.getByText(/tailscale 1\.98\.10 → 1\.104\.2/)).toBeTruthy();
  });

  it("posts once on Check now and shows Running…", async () => {
    const actions = fakeActions();
    actions.runUpdates.mockImplementation(() => new Promise(() => {}));
    ui(updates(), me({}), actions);
    fireEvent.click(screen.getByRole("button", { name: "Check now" }));
    const b = (await screen.findByRole("button", { name: /Running/ })) as HTMLButtonElement;
    expect(b.disabled).toBe(true);
    fireEvent.click(b);
    expect(actions.runUpdates).toHaveBeenCalledTimes(1);
  });

  it("shows Running… while a run is going", () => {
    ui(updates({ running: true }));
    expect((screen.getByRole("button", { name: /Running/ }) as HTMLButtonElement).disabled).toBe(true);
  });

  it("is hidden from readers' buttons and disabled off the tailnet", () => {
    ui(updates(), me({ role: "reader", can_act: false, reason: "not_admin" }));
    expect(screen.queryByRole("button", { name: "Check now" })).toBeNull();
    cleanup();
    ui(updates(), me({ can_act: false, reason: "not_tailnet" }));
    const b = screen.getByRole("button", { name: "Check now" }) as HTMLButtonElement;
    expect(b.disabled).toBe(true);
    expect(b.title).toBe("Only over Tailscale");
  });

  it("shows a check error for a software", () => {
    ui(updates({ software: [sw({ latest: null, error: "rate limited" })] }));
    expect(screen.getByText(/rate limited/)).toBeTruthy();
  });
});
