// @vitest-environment jsdom
// Components render the right state, label and icon, and call the right action. Data comes
// from a fake ActionsContext, the way connection/hooks hands it to them.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import type { ComponentChildren } from "preact";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { ChannelView } from "@/bindings/ChannelView";
import type { HostView } from "@/bindings/HostView";
import { ThemeProvider } from "@/components/brand/ThemeProvider";
import { ThemeToggle } from "@/components/brand/ThemeToggle";
import { ChannelCard } from "@/components/fleet/ChannelCard";
import { HostRow } from "@/components/fleet/HostRow";
import { OnlineState } from "@/components/fleet/OnlineState";
import { PromoteDialog } from "@/components/fleet/PromoteCard";
import { StatusBadge } from "@/components/fleet/StatusBadge";
import { TooltipProvider } from "@/components/ui/tooltip";
import { type Actions, ActionsContext } from "@/connection/ConnectionProvider";
import { BrandIcon } from "@/icons/BrandIcon";
import { readTheme, THEME_KEY, writeTheme } from "@/lib/theme";

const REV = "0123456789abcdef0123456789abcdef01234567";
const NOW = 10_000;

function apply(over: Partial<ApplyRecord>): ApplyRecord {
  return {
    apply_id: "a1",
    commit: REV,
    state: "ok",
    skip_reason: null,
    message: null,
    exit_code: null,
    started_at: NOW - 600,
    finished_at: NOW - 540,
    ...over,
  };
}

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

function fakeActions(): Actions & { [K in keyof Actions]: ReturnType<typeof vi.fn> } {
  return {
    applyHost: vi.fn(async () => {}),
    applyChannel: vi.fn(async () => {}),
    promote: vi.fn(async () => true),
    showLog: vi.fn(),
    closeLog: vi.fn(),
    fetchLog: vi.fn(),
  } as never;
}

function withActions(actions: Actions, children: ComponentChildren) {
  return (
    <ActionsContext.Provider value={actions}>
      <TooltipProvider>{children}</TooltipProvider>
    </ActionsContext.Provider>
  );
}

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

/** The Lucide icon in an element, by its `lucide-<name>` class. */
const iconOf = (el: Element) =>
  [...(el.querySelector("svg")?.classList ?? [])].find((c) => c.startsWith("lucide-") && c !== "lucide-icon");

afterEach(() => {
  cleanup();
  document.documentElement.removeAttribute("data-theme");
  localStorage.clear();
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

describe("HostRow", () => {
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

  it("opens the log of the last apply from an icon button with a label", () => {
    const { actions } = renderRow(host({ last_apply: apply({ apply_id: "a7" }) }));
    fireEvent.click(screen.getByRole("button", { name: "Show log for desktop" }));
    expect(actions.showLog).toHaveBeenCalledWith("desktop", "a7");
  });

  it("has no log button before the first apply", () => {
    renderRow(host({ last_apply: null }));
    expect(screen.queryByRole("button", { name: /Show log/ })).toBeNull();
    expect(screen.getByText("Never applied")).toBeTruthy();
  });

  it("applies newest on click, and not while an apply runs", async () => {
    const { actions } = renderRow(host({ behind: 2 }));
    fireEvent.click(screen.getByRole("button", { name: "Apply newest" }));
    expect(actions.applyHost).toHaveBeenCalledWith("desktop");
    cleanup();
    renderRow(host({ last_apply: apply({ state: "running", finished_at: null }) }));
    const b = screen.getByRole("button", { name: "Applying" }) as HTMLButtonElement;
    expect(b.disabled).toBe(true);
  });

  it("says when an apply is queued for a machine that is off", () => {
    renderRow(host({ online: "off", desired_apply_id: "a9", last_apply: apply({ apply_id: "a1" }) }));
    expect(screen.getByText("Queued: sent when desktop comes online")).toBeTruthy();
  });
});

describe("ChannelCard", () => {
  const channel = (over: Partial<ChannelView>): ChannelView => ({
    name: "unstable",
    head: REV,
    head_subject: "fix(users): linger",
    ...over,
  });

  it("shows the head and how many machines are behind; applies to the channel", () => {
    const actions = fakeActions();
    const { container } = render(
      withActions(
        actions,
        <ChannelCard
          channel={channel({})}
          hosts={[host({ name: "a", behind: 0 }), host({ name: "b", behind: 2 }), host({ name: "c", behind: null })]}
          role="unstable"
        />,
      ),
    );
    expect(container.textContent).toContain("3 machines · 1 behind");
    expect(screen.getByText("0123456")).toBeTruthy();
    expect(screen.getByText("fix(users): linger")).toBeTruthy();
    expect(container.querySelector("[data-role=unstable]")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "Apply newest to all on unstable" }));
    expect(actions.applyChannel).toHaveBeenCalledWith("unstable");
  });

  it("can't apply without a head or without machines", () => {
    render(withActions(fakeActions(), <ChannelCard channel={channel({ head: null })} hosts={[host({})]} role="stable" />));
    expect(screen.getByText("No head: branch not in the mirror")).toBeTruthy();
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
    cleanup();
    render(withActions(fakeActions(), <ChannelCard channel={channel({})} hosts={[]} role="stable" />));
    expect((screen.getByRole("button") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("PromoteDialog", () => {
  it("promotes the commit it showed when it opened, even if the candidate moved since", async () => {
    const actions = fakeActions();
    const pair = { from: "unstable", to: "stable" };
    const channels: ChannelView[] = [
      { name: "unstable", head: REV, head_subject: "first" },
      { name: "stable", head: "f".repeat(40), head_subject: "old" },
    ];
    const ui = (candidate: string) =>
      withActions(actions, <PromoteDialog candidate={candidate} pair={pair} channels={channels} />);
    const { rerender } = render(ui(REV));
    fireEvent.click(screen.getByRole("button", { name: "Promote 0123456" }));
    expect(await screen.findByRole("dialog")).toBeTruthy();
    rerender(ui("a".repeat(40)));
    fireEvent.click(screen.getAllByRole("button", { name: "Promote 0123456" }).at(-1)!);
    await vi.waitFor(() => expect(actions.promote).toHaveBeenCalledWith(REV, pair));
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

describe("ThemeToggle", () => {
  it("sets data-theme on <html> and remembers the choice; system clears both", () => {
    render(
      <ThemeProvider>
        <ThemeToggle />
      </ThemeProvider>,
    );
    const root = document.documentElement;
    fireEvent.click(screen.getByRole("button", { name: "Dark theme" }));
    expect(root.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem(THEME_KEY)).toBe("dark");
    expect(screen.getByRole("button", { name: "Dark theme" }).getAttribute("aria-pressed")).toBe("true");
    fireEvent.click(screen.getByRole("button", { name: "Light theme" }));
    expect(root.getAttribute("data-theme")).toBe("light");
    fireEvent.click(screen.getByRole("button", { name: "Use the system theme" }));
    expect(root.hasAttribute("data-theme")).toBe(false);
    expect(localStorage.getItem(THEME_KEY)).toBeNull();
  });

  it("works when storage throws or holds junk", () => {
    const broken = {
      getItem: () => {
        throw new Error("blocked");
      },
      setItem: () => {
        throw new Error("blocked");
      },
      removeItem: () => {
        throw new Error("blocked");
      },
    };
    expect(readTheme(broken)).toBe("system");
    expect(() => writeTheme("dark", broken)).not.toThrow();
    expect(readTheme({ getItem: () => "sepia" })).toBe("system");
    expect(readTheme({ getItem: () => "light" })).toBe("light");
  });
});
