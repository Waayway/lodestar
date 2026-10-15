// @vitest-environment jsdom
// Services in the connection layer: the hash route, the reducer, and the host-online join.
import { cleanup, render, screen } from "@testing-library/preact";
import { useMemo } from "preact/hooks";
import { afterEach, describe, expect, it } from "vitest";
import type { Snapshot } from "@/bindings/Snapshot";
import { reduceSnapshot, StateContext } from "@/connection/ConnectionProvider";
import { useServices } from "@/connection/hooks";
import { parseRoute } from "@/lib/route";
import { host, service } from "./helpers";

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
  const state = useMemo(() => ({ snapshot: snap, feed: { kind: "live" } as never, openLog: null, audit: null }), [snap]);
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
