// @vitest-environment jsdom
// State display: every state shows the right colour, icon and words.
import { act, cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { HostView } from "@/bindings/HostView";
import { BehindCount } from "@/components/fleet/BehindCount";
import { HostRow } from "@/components/fleet/HostRow";
import { HostTable } from "@/components/fleet/HostTable";
import { LivePill } from "@/components/fleet/LivePill";
import { OnlineState } from "@/components/fleet/OnlineState";
import { StatusBadge } from "@/components/fleet/StatusBadge";
import { BrandIcon } from "@/icons/BrandIcon";
import { apply, fakeActions, host, iconOf, NOW, REV, withActions } from "./helpers";

function renderRow(h: HostView, actions = fakeActions()) {
  const r = render(
    withActions(
      actions,
      <table>
        <tbody>
          <HostRow host={h} now={NOW} channelRole="unstable" />
        </tbody>
      </table>,
    ),
  );
  return { ...r, actions };
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("StatusBadge", () => {
  const cases: [ApplyRecord | null, string, string, string][] = [
    [apply({ state: "ok" }), "ok", "Applied", "lucide-check"],
    [apply({ state: "running", finished_at: null }), "running", "Applying", "lucide-rotate-cw"],
    [apply({ state: "skipped", skip_reason: "dirty" }), "skipped", "Skipped: local changes in the checkout", "lucide-pen-line"],
    [apply({ state: "failed", exit_code: 1, message: "nix build failed" }), "failed", "Build failed (exit 1)", "lucide-circle-x"],
    [null, "idle", "Never applied", "lucide-circle-dashed"],
  ];
  for (const [rec, state, text, icon] of cases) {
    it(`${state}: "${text}"`, () => {
      const { container } = render(<StatusBadge apply={rec} />);
      const badge = container.querySelector("[data-slot=badge]")!;
      expect(badge.getAttribute("data-state")).toBe(state);
      expect(badge.textContent).toBe(text);
      expect(iconOf(badge)).toBe(icon);
    });
  }
  it("spins only while building", () => {
    const running = render(<StatusBadge apply={apply({ state: "running" })} />).container;
    expect(running.querySelector("svg")!.classList.contains("animate-spin")).toBe(true);
    cleanup();
    const ok = render(<StatusBadge apply={apply({ state: "ok" })} />).container;
    expect(ok.querySelector("svg")!.classList.contains("animate-spin")).toBe(false);
  });
  it("names the checkout path when skipped for local changes", () => {
    render(<StatusBadge apply={apply({ state: "skipped", skip_reason: "dirty" })} flakePath="/home/alice/infra" />);
    expect(screen.getByText("Skipped: local changes in /home/alice/infra")).toBeTruthy();
  });
});

describe("OnlineState", () => {
  it("online: a dot and Online", () => {
    const { container } = render(<OnlineState host={{ online: "on", last_seen: null }} now={NOW} />);
    expect(container.textContent).toBe("Online");
    expect(container.querySelector("svg")).toBeNull();
  });
  it("agent not answering: warn colour and the wifi-off icon", () => {
    const { container } = render(<OnlineState host={{ online: "agent_unreachable", last_seen: null }} now={NOW} />);
    const el = container.firstElementChild!;
    expect(el.textContent).toBe("Agent not answering");
    expect(el.classList.contains("text-warn")).toBe(true);
    expect(iconOf(el)).toBe("lucide-wifi-off");
  });
  it("off: muted, the power icon and when it was last seen", () => {
    const { container } = render(<OnlineState host={{ online: "off", last_seen: NOW - 7200 }} now={NOW} />);
    const el = container.firstElementChild!;
    expect(el.textContent).toBe("Off · last seen 2h ago");
    expect(el.classList.contains("text-muted")).toBe(true);
    expect(iconOf(el)).toBe("lucide-power");
  });
});

describe("HostRow display", () => {
  it("shows the machine, its channel, state, running commit and last apply", () => {
    renderRow(
      host({
        os: "darwin",
        running: `${REV}-dirty`,
        running_dirty: true,
        behind: 3,
        last_apply: apply({ state: "failed", exit_code: 1, message: "nix build failed" }),
      }),
    );
    const row = document.querySelector("tr")!;
    expect(row.getAttribute("data-tone")).toBe("failed");
    expect(screen.getByText("desktop")).toBeTruthy();
    expect(screen.getByText("macOS")).toBeTruthy();
    expect(screen.getByRole("img", { name: "macOS" })).toBeTruthy();
    expect(row.querySelector("[data-role=unstable]")!.textContent).toBe("unstable");
    expect(screen.getByText("dirty")).toBeTruthy();
    expect(screen.getByText("3 behind").classList.contains("text-warn")).toBe(true);
    expect(row.querySelector("[data-slot=badge]")!.getAttribute("data-state")).toBe("failed");
    expect(screen.getByText("Build failed (exit 1)")).toBeTruthy();
  });

  it("draws the state stripe from hostTone, none for an idle machine", () => {
    renderRow(host({ online: "off", last_apply: apply({ state: "failed" }) }));
    let cell = document.querySelector("td")!;
    expect(document.querySelector("tr")!.getAttribute("data-tone")).toBe("failed");
    expect(cell.className).toContain("shadow-[inset_3px_0_0_var(--bad)]");
    cleanup();
    renderRow(host({ behind: 2 }));
    cell = document.querySelector("td")!;
    expect(cell.className).not.toContain("shadow-");
  });
});

describe("BehindCount", () => {
  it("is green at zero, amber behind, muted unknown", () => {
    const { container } = render(
      <>
        <BehindCount behind={0} />
        <BehindCount behind={4} />
        <BehindCount behind={null} />
      </>,
    );
    const [zero, some, unknown] = container.querySelectorAll("span");
    expect([zero.textContent, zero.className.includes("text-ok")]).toEqual(["up to date", true]);
    expect([some.textContent, some.className.includes("text-warn")]).toEqual(["4 behind", true]);
    expect([unknown.textContent, unknown.className.includes("text-muted")]).toEqual(["unknown", true]);
  });
});

describe("LivePill", () => {
  it("counts down visibly but announces only the state", async () => {
    vi.useFakeTimers();
    const retryAt = Date.now() + 5000;
    render(<LivePill feed={{ kind: "waiting", retryAt }} />);
    const status = screen.getByRole("status");
    expect(status.textContent).toBe("Disconnected, retrying");
    expect(document.body.textContent).toContain("Disconnected · retry in 5s");
    await act(() => {
      vi.advanceTimersByTime(2000);
    });
    expect(document.body.textContent).toContain("Disconnected · retry in 3s");
    expect(screen.getByRole("status").textContent).toBe("Disconnected, retrying");
    expect(screen.getByText(/retry in 3s/).getAttribute("aria-hidden")).toBe("true");
  });
  it("live and no access", () => {
    render(<LivePill feed={{ kind: "live" }} />);
    expect(screen.getByRole("status").textContent).toBe("Live");
    cleanup();
    render(<LivePill feed={{ kind: "forbidden" }} />);
    expect(screen.getByRole("status").textContent).toBe("No access");
  });
});

describe("BrandIcon", () => {
  it("draws NixOS in --nix and Apple in the text colour, labelled", () => {
    const { container } = render(
      <>
        <BrandIcon brand="nixos" />
        <BrandIcon brand="apple" />
      </>,
    );
    const [nix, apple] = container.querySelectorAll("svg");
    expect(nix.getAttribute("aria-label")).toBe("NixOS");
    expect(nix.classList.contains("text-nix")).toBe(true);
    expect(apple.getAttribute("aria-label")).toBe("macOS");
    expect(apple.getAttribute("fill")).toBe("currentColor");
    expect(nix.querySelector("path")!.getAttribute("d")!.length).toBeGreaterThan(50);
  });
});

describe("Machines layout", () => {
  const hosts: HostView[] = [
    host({ name: "nixos-manager", channel: "stable", last_activation: { line: 1, time: NOW - 7200, store_path: "/nix/store/x", revision: REV, generation: 11, trigger: "apply:a1", sudo_user: null } }),
    host({ name: "laptop", os: "darwin", last_apply: apply({ state: "failed", exit_code: 1, message: "nix build failed" }) }),
  ];
  const renderTable = () =>
    render(withActions(fakeActions(), <HostTable hosts={hosts} now={NOW} promote={{ from: "unstable", to: "stable" }} />));

  it("is a six-column table from 900 px and a card list below, with no minimum width", () => {
    const { container } = renderTable();
    const box = container.querySelector("[data-slot=table-container]")!;
    expect(box.className).toContain("hidden");
    expect(box.className).toContain("min-[900px]:block");
    const table = box.querySelector("table")!;
    expect(table.className).not.toMatch(/min-w-/);
    expect([...table.querySelectorAll("th")].map((th) => th.textContent)).toEqual([
      "Machine",
      "Status",
      "Running",
      "Behind",
      "Last apply",
      "Actions",
    ]);
    expect(table.querySelectorAll("tbody tr")[0].querySelectorAll("td")).toHaveLength(6);
    const cards = container.querySelector("[data-layout=cards]")!;
    expect(cards.className).toContain("min-[900px]:hidden");
    // By channel, then name, like the table.
    expect([...cards.querySelectorAll("li")].map((li) => li.getAttribute("data-host"))).toEqual([
      "nixos-manager",
      "laptop",
    ]);
  });

  it("keeps names whole and shows the channel under the name in both layouts", () => {
    const { container } = renderTable();
    for (const scope of [container.querySelector("table")!, container.querySelector("[data-layout=cards]")!]) {
      const row = scope.querySelector("[data-host=nixos-manager]")!;
      const name = [...row.querySelectorAll("div")].find((d) => d.textContent === "nixos-manager")!;
      expect(name.className).toContain("whitespace-nowrap");
      expect(row.querySelector("[data-role=stable]")!.textContent).toBe("stable");
    }
  });

  it("cards show the last activation and a full-width apply button", () => {
    const { container } = renderTable();
    const card = container.querySelector("[data-layout=cards] [data-host=nixos-manager]")!;
    expect(card.textContent).toContain("Last activation");
    expect(card.textContent).toContain("2h ago · 0123456 · Lodestar apply · gen 11");
    const applyBtn = [...card.querySelectorAll("button")].find((b) => b.textContent === "Apply newest")!;
    expect(applyBtn.className).toContain("flex-1");
  });

  it("the table row's badge tooltip carries the message and the last activation", async () => {
    // jsdom has no ResizeObserver; Radix's popper measures the tooltip with one.
    vi.stubGlobal(
      "ResizeObserver",
      class {
        observe() {}
        unobserve() {}
        disconnect() {}
      },
    );
    const { container } = renderTable();
    const row = container.querySelector("table [data-host=laptop]")!;
    const trigger = row.querySelector("[data-slot=badge]")!.parentElement!;
    fireEvent.pointerMove(trigger, { pointerType: "mouse" });
    const tip = await screen.findByRole("tooltip");
    expect(tip.textContent).toContain("Build failed (exit 1)");
    expect(tip.textContent).toContain("nix build failed");
    expect(tip.textContent).toContain("Last activation: none recorded");
  });
});
