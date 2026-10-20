// @vitest-environment jsdom
// Services in the connection layer: the hash route, the reducer, and the host-online join.
import { cleanup, render, screen } from "@testing-library/preact";
import { useMemo } from "preact/hooks";
import { afterEach, describe, expect, it } from "vitest";
import type { Snapshot } from "@/bindings/Snapshot";
import { ActionsContext, reduceSnapshot, StateContext } from "@/connection/ConnectionProvider";
import { useServices } from "@/connection/hooks";
import type { ComponentChildren } from "preact";
import type { ServiceView } from "@/bindings/ServiceView";
import { NavTabs } from "@/components/layout/NavTabs";
import { ServiceCard } from "@/components/services/ServiceCard";
import { TooltipProvider } from "@/components/ui/tooltip";
import { Building, Down, Idle, Off, Unreachable, Up } from "@/icons";
import { downReasonLabel, groupServices, serviceCounts, serviceLabel, serviceSubLine } from "@/lib/format";
import { parseRoute } from "@/lib/route";
import { serviceStyle } from "@/lib/tones";
import { groupColumns, ServicesPage } from "@/pages/ServicesPage";
import { fakeActions, host, NOW, service } from "./helpers";

afterEach(cleanup);

const snapshot = (over: Partial<Snapshot> = {}): Snapshot => ({
  hosts: [host({ name: "desktop" })],
  channels: [],
  promote_candidate: null,
  promote: null,
  viewer: "tester",
  demo: false,
  services: [],
  ...over,
});

describe("parseRoute", () => {
  it("maps #/services to services and everything else to fleet", () => {
    expect(parseRoute("#/services")).toBe("services");
    expect(parseRoute("")).toBe("fleet");
    expect(parseRoute("#/")).toBe("fleet");
    expect(parseRoute("#/nope")).toBe("fleet");
  });
});

describe("reduceSnapshot services", () => {
  it("replaces the list and keeps the hosts", () => {
    const s = snapshot({ services: [service({ id: "old" })] });
    const next = reduceSnapshot(s, { type: "services", data: [service({ id: "new" })] });
    expect(next?.services.map((x) => x.id)).toEqual(["new"]);
    expect(next?.hosts).toBe(s.hosts);
  });
  it("ignores services before the first snapshot", () => {
    expect(reduceSnapshot(null, { type: "services", data: [] })).toBeNull();
  });
});

function Probe({ snap }: { snap: Snapshot | null }) {
  const state = useMemo(() => ({ snapshot: snap, feed: { kind: "live" } as never, openLog: null, audit: null, viewer: null, mode: "signed_in" as const, meRetryAt: null, publicServices: null }), [snap]);
  return (
    <StateContext.Provider value={state}>
      <Inner />
    </StateContext.Provider>
  );
}
function Inner() {
  const list = useServices();
  return <p data-testid="out">{list === null ? "null" : list.map((x) => `${x.id}:${x.host_online}`).join(",")}</p>;
}

describe("useServices", () => {
  it("is null before the first snapshot", () => {
    render(<Probe snap={null} />);
    expect(screen.getByTestId("out").textContent).toBe("null");
  });
  it("takes online state from the current host list, so a host update alone moves the cards", () => {
    const s = snapshot({
      services: [
        service({ id: "a", host: "desktop", host_online: "on" }),
        service({ id: "b", host: "elsewhere", host_online: "off" }),
      ],
    });
    const { rerender } = render(<Probe snap={s} />);
    expect(screen.getByTestId("out").textContent).toBe("a:on,b:off");
    const went = reduceSnapshot(s, { type: "host", data: host({ name: "desktop", online: "off" }) });
    rerender(<Probe snap={went} />);
    expect(screen.getByTestId("out").textContent).toBe("a:off,b:off");
  });
});

// ---- Task 8: the Services tab ----

const at = (over: Partial<ServiceView["check"]>): ServiceView["check"] => ({
  state: "up",
  reason: null,
  latency_ms: 84,
  checked_at: NOW - 30,
  since: NOW - 7200,
  ...over,
});

describe("serviceStyle", () => {
  it.each([
    ["up", service({ check: at({}) }), "ok", Up, false],
    ["up but slow", service({ check: at({ latency_ms: 1200 }) }), "warn", Up, false],
    ["down", service({ check: at({ state: "down", reason: { kind: "timeout" } }) }), "bad", Down, false],
    ["down, host off", service({ host_online: "off", check: at({ state: "down" }) }), "warn", Off, false],
    [
      "down, agent not answering",
      service({ host_online: "agent_unreachable", check: at({ state: "down" }) }),
      "warn",
      Unreachable,
      false,
    ],
    ["pending", service({ check: at({ state: "pending", latency_ms: null, checked_at: null }) }), "info", Building, true],
    ["unchecked", service({ check: at({ state: "unchecked", latency_ms: null }) }), "neutral", Idle, false],
  ] as const)("%s", (_name, s, tone, icon, spin) => {
    const st = serviceStyle(s);
    expect(st.tone).toBe(tone);
    expect(st.icon).toBe(icon);
    expect(!!st.spin).toBe(spin);
  });
});

describe("serviceLabel and downReasonLabel", () => {
  it("words every state", () => {
    expect(serviceLabel(service({ check: at({}) }))).toBe("Up · 84 ms");
    expect(serviceLabel(service({ check: at({ state: "down", reason: { kind: "http_status", detail: 502 } }) }))).toBe(
      "Down: HTTP 502",
    );
    expect(serviceLabel(service({ host_online: "off", check: at({ state: "down", reason: { kind: "timeout" } }) }))).toBe(
      "Down · host off",
    );
    expect(serviceLabel(service({ host_online: "agent_unreachable", check: at({ state: "down" }) }))).toBe(
      "Down · agent not answering",
    );
    expect(serviceLabel(service({ check: at({ state: "pending" }) }))).toBe("Checking…");
    expect(serviceLabel(service({ check: at({ state: "unchecked" }) }))).toBe("Not checked");
  });
  it("names every reason", () => {
    expect(downReasonLabel({ kind: "timeout" })).toBe("timed out");
    expect(downReasonLabel({ kind: "connection_refused" })).toBe("connection refused");
    expect(downReasonLabel({ kind: "unreachable" })).toBe("unreachable");
    expect(downReasonLabel({ kind: "tls" })).toBe("TLS error");
    expect(downReasonLabel({ kind: "http_status", detail: 502 })).toBe("HTTP 502");
    expect(downReasonLabel({ kind: "other", detail: "dns failed" })).toBe("dns failed");
  });
  it("says how long the service has been in its state", () => {
    expect(serviceSubLine(service({ check: at({}) }), NOW)).toBe("up since 2h");
    expect(serviceSubLine(service({ check: at({ since: NOW - 300 }) }), NOW)).toBe("up since 5m");
    expect(serviceSubLine(service({ check: at({ since: NOW - 10 }) }), NOW)).toBe("up just now");
    expect(serviceSubLine(service({ check: at({ state: "down" }) }), NOW)).toBe("down since 2h");
    expect(serviceSubLine(service({ check: at({ state: "down", since: NOW - 5 }) }), NOW)).toBe("down just now");
    expect(serviceSubLine(service({ check: at({ state: "pending", checked_at: null }) }), NOW)).toBeNull();
    expect(serviceSubLine(service({ check: at({ state: "unchecked", since: null }) }), NOW)).toBeNull();
  });
});

describe("groupServices and serviceCounts", () => {
  const list = [
    service({ id: "c", title: "Zulu", group: "Media" }),
    service({ id: "a", title: "Alpha", group: "Monitoring", check: at({ state: "down" }) }),
    service({ id: "b", title: "Bravo", group: "Media", check: at({ state: "pending" }) }),
    service({ id: "d", title: "Delta", group: "Apps", check: at({ state: "unchecked" }) }),
  ];
  it("groups by name, cards by title", () => {
    expect(groupServices(list).map(([g, xs]) => [g, xs.map((x) => x.title)])).toEqual([
      ["Apps", ["Delta"]],
      ["Media", ["Bravo", "Zulu"]],
      ["Monitoring", ["Alpha"]],
    ]);
  });
  it("counts each state", () => {
    expect(serviceCounts(list)).toEqual({ up: 1, down: 1, pending: 1, unchecked: 1 });
  });
});

describe("ServiceCard", () => {
  const long = "https://a-very-long-host-name.example.test/with/a/very/long/path/that/never/fits/on/a/phone";
  it("links the title out, truncates the URL, and shows the state", () => {
    const { container } = render(
      <TooltipProvider>
        <ul>
          <ServiceCard
            service={service({ url: long, check: at({ state: "down", reason: { kind: "http_status", detail: 502 } }) })}
            now={NOW}
          />
        </ul>
      </TooltipProvider>,
    );
    const link = screen.getByRole("link", { name: /Grafana/ });
    expect(link.getAttribute("href")).toBe(long);
    expect(link.getAttribute("target")).toBe("_blank");
    expect(link.getAttribute("rel")).toContain("noopener");
    const url = screen.getByText(long);
    expect(url.className).toContain("truncate");
    expect(url.getAttribute("title")).toBe(long);
    expect(screen.getByText("Down: HTTP 502")).toBeTruthy();
    expect(screen.getByText("down since 2h")).toBeTruthy();
    const card = container.querySelector("[data-tone]")!;
    expect(card.getAttribute("data-tone")).toBe("bad");
    expect(card.className).toContain("min-w-0");
  });
  it("draws a known brand, a Lodestar icon, or the first letter", () => {
    render(
      <TooltipProvider>
        <ul>
          <ServiceCard service={service({ id: "j", title: "Jellyfin", icon: "jellyfin" })} now={NOW} />
          <ServiceCard service={service({ id: "m", title: "Box", icon: "Machines" })} now={NOW} />
          <ServiceCard service={service({ id: "r", title: "restic", icon: "restic" })} now={NOW} />
        </ul>
      </TooltipProvider>,
    );
    expect(document.querySelector('[data-brand="jellyfin"]')).toBeTruthy();
    expect(document.querySelector(".lucide-monitor")).toBeTruthy();
    expect(screen.getByText("r")).toBeTruthy();
  });
});

function WithSnap({ snap, children }: { snap: Snapshot | null; children: ComponentChildren }) {
  const state = useMemo(() => ({ snapshot: snap, feed: { kind: "live" } as never, openLog: null, audit: null, viewer: null, mode: "signed_in" as const, meRetryAt: null, publicServices: null }), [snap]);
  return (
    <StateContext.Provider value={state}>
      <ActionsContext.Provider value={fakeActions()}>
        <TooltipProvider>{children}</TooltipProvider>
      </ActionsContext.Provider>
    </StateContext.Provider>
  );
}

describe("ServicesPage", () => {
  it("shows the empty state without services", () => {
    render(
      <WithSnap snap={snapshot()}>
        <ServicesPage />
      </WithSnap>,
    );
    expect(screen.getByText("No services declared")).toBeTruthy();
  });
  it("shows one section per group, in order", () => {
    render(
      <WithSnap
        snap={snapshot({
          services: [service({ id: "b", group: "Monitoring" }), service({ id: "a", title: "Jellyfin", group: "Media" })],
        })}
      >
        <ServicesPage />
      </WithSnap>,
    );
    const headings = screen.getAllByRole("heading", { level: 2 }).map((h) => h.textContent ?? "");
    expect(headings).toHaveLength(2);
    expect(headings[0]).toMatch(/^Media/);
    expect(headings[1]).toMatch(/^Monitoring/);
  });
  it("packs groups densely, each spanning as many columns as it has cards", () => {
    render(
      <WithSnap
        snap={snapshot({
          services: [
            service({ id: "a", group: "Apps" }),
            ...["p", "q", "r", "s", "t"].map((id) => service({ id, title: id, group: "Infrastructure" })),
          ],
        })}
      >
        <ServicesPage />
      </WithSnap>,
    );
    const apps = screen.getByRole("region", { name: "Apps" });
    const infrastructure = screen.getByRole("region", { name: "Infrastructure" });
    expect(apps.parentElement?.className).toContain("grid-flow-row-dense");
    expect(apps.className).toContain(groupColumns(1).section);
    expect(infrastructure.className).toContain(groupColumns(5).section);
    expect(infrastructure.querySelector("ul")?.className).toContain(groupColumns(5).list);
  });
});

describe("groupColumns", () => {
  it("spans one column per card, capped at the columns of each breakpoint", () => {
    expect(groupColumns(1).section.split(" ")).toEqual([
      "col-span-1",
      "min-[640px]:col-span-1",
      "min-[900px]:col-span-1",
      "min-[1200px]:col-span-1",
    ]);
    expect(groupColumns(3).section.split(" ")).toEqual([
      "col-span-1",
      "min-[640px]:col-span-2",
      "min-[900px]:col-span-3",
      "min-[1200px]:col-span-3",
    ]);
    expect(groupColumns(9).list.split(" ")).toEqual([
      "grid-cols-1",
      "min-[640px]:grid-cols-2",
      "min-[900px]:grid-cols-3",
      "min-[1200px]:grid-cols-4",
    ]);
  });
});

describe("NavTabs", () => {
  it("marks the current tab and shows the down count", () => {
    render(<NavTabs route="services" downCount={3} />);
    const services = screen.getByRole("link", { name: /Services/ });
    const fleet = screen.getByRole("link", { name: /Fleet/ });
    expect(services.getAttribute("href")).toBe("#/services");
    expect(fleet.getAttribute("href")).toBe("#/");
    expect(services.getAttribute("aria-current")).toBe("page");
    expect(fleet.getAttribute("aria-current")).toBeNull();
    expect(services.textContent).toContain("3");
  });
  it("has no count badge when nothing is down", () => {
    render(<NavTabs route="fleet" downCount={0} />);
    expect(screen.getByRole("link", { name: /Fleet/ }).getAttribute("aria-current")).toBe("page");
    expect(screen.getByRole("link", { name: /Services/ }).textContent).not.toMatch(/\d/);
  });
});
