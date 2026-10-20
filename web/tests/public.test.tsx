// @vitest-environment jsdom
// The anonymous Services page, the header's Log in / Log out, and the public-service helpers.
import { cleanup, fireEvent, render, screen } from "@testing-library/preact";
import { afterEach, describe, expect, it } from "vitest";
import { PageHeader } from "@/components/layout/PageHeader";
import { NavTabs } from "@/components/layout/NavTabs";
import { groupServices, publicServiceCounts, publicServiceLabel, stateSince } from "@/lib/format";
import { publicServiceStyle } from "@/lib/tones";
import { PublicServicesPage } from "@/pages/PublicServicesPage";
import { fakeActions, NOW, publicService, withActions } from "./helpers";

afterEach(cleanup);

const anonymous = (list: ReturnType<typeof publicService>[] | null) => ({
  viewer: { viewer: null, role: "anonymous" as const, session: false, can_act: false, reason: "anonymous" as const },
  mode: "anonymous" as const,
  publicServices: list,
});

describe("PublicServicesPage", () => {
  const two = [
    publicService({ title: "Jellyfin", group: "Media", description: "Films and shows", state: "unknown", since: null }),
    publicService({ title: "Grafana", group: "Monitoring", description: "Dashboards" }),
  ];

  it("shows titles, descriptions and states, without any link inside the cards", () => {
    const { container } = render(withActions(fakeActions(), <PublicServicesPage />, anonymous(two)));
    expect(screen.getByText("Grafana")).toBeTruthy();
    expect(screen.getByText("Jellyfin")).toBeTruthy();
    expect(screen.getByText("Dashboards")).toBeTruthy();
    expect(screen.getByText("Films and shows")).toBeTruthy();
    expect(container.querySelectorAll("li [data-state=up]")).toHaveLength(1);
    expect(container.querySelector("li [data-state=up]")?.textContent).toBe("Up");
    expect(container.querySelector("li [data-state=unknown]")?.textContent).toBe("Unknown");
    expect(container.querySelectorAll("li a")).toHaveLength(0);
    expect(container.textContent).not.toMatch(/https?:|\.example/);
  });

  it("has only the Services tab", () => {
    render(withActions(fakeActions(), <PublicServicesPage />, anonymous(two)));
    expect(screen.queryByText("Fleet")).toBeNull();
    expect(screen.getByRole("link", { name: /Services/ })).toBeTruthy();
  });

  it("logs in from the header button", () => {
    const actions = fakeActions();
    render(withActions(actions, <PublicServicesPage />, anonymous(two)));
    fireEvent.click(screen.getByRole("button", { name: "Log in" }));
    expect(actions.login).toHaveBeenCalledTimes(1);
  });

  it("says when there are no services", () => {
    render(withActions(fakeActions(), <PublicServicesPage />, anonymous([])));
    expect(screen.getByText("No services declared")).toBeTruthy();
  });
});

describe("PageHeader session", () => {
  const header = (session: boolean) => (
    <PageHeader feed={{ kind: "live" } as never} viewer="bob" session={session} hostCount={1} route="fleet" downCount={0} />
  );

  it("shows Log out for a session and calls logout", () => {
    const actions = fakeActions();
    render(withActions(actions, header(true)));
    fireEvent.click(screen.getByRole("button", { name: "Log out" }));
    expect(actions.logout).toHaveBeenCalledTimes(1);
  });

  it("shows no Log out without a session", () => {
    render(withActions(fakeActions(), header(false)));
    expect(screen.queryByRole("button", { name: "Log out" })).toBeNull();
  });
});

describe("NavTabs fleet", () => {
  it("hides the Fleet tab when fleet is false", () => {
    render(<NavTabs route="services" downCount={0} fleet={false} />);
    expect(screen.queryByText("Fleet")).toBeNull();
  });
});

describe("public service helpers", () => {
  it("labels and styles the three states", () => {
    expect(publicServiceLabel("up")).toBe("Up");
    expect(publicServiceLabel("down")).toBe("Down");
    expect(publicServiceLabel("unknown")).toBe("Unknown");
    expect(publicServiceStyle("up").tone).toBe("ok");
    expect(publicServiceStyle("down").tone).toBe("bad");
    expect(publicServiceStyle("unknown").tone).toBe("neutral");
  });

  it("counts by state", () => {
    const list = [publicService({}), publicService({ state: "down" }), publicService({ state: "unknown" }), publicService({})];
    expect(publicServiceCounts(list)).toEqual({ up: 2, down: 1, unknown: 1 });
  });

  it("groups any list by group then title", () => {
    const list = [
      { group: "B", title: "z" },
      { group: "A", title: "y" },
      { group: "A", title: "x" },
    ];
    expect(groupServices(list)).toEqual([
      ["A", [list[2], list[1]]],
      ["B", [list[0]]],
    ]);
  });

  it("words how long a state has held", () => {
    expect(stateSince("up", NOW - 3 * 3600, NOW)).toBe("up since 3h");
    expect(stateSince("down", NOW - 5, NOW)).toBe("down just now");
    expect(stateSince("unknown", NOW - 100, NOW)).toBeNull();
    expect(stateSince("up", null, NOW)).toBeNull();
  });
});
