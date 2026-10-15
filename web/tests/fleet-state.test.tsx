// @vitest-environment jsdom
// Fleet state: how feed messages change the snapshot, the promote candidate refresh (newest
// answer wins) and the debounced audit refresh. Drives ConnectionProvider through a fake
// WebSocket and a fake fetch.
import { act, cleanup, render } from "@testing-library/preact";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { ChannelView } from "@/bindings/ChannelView";
import type { Snapshot } from "@/bindings/Snapshot";
import type { WsMessage } from "@/bindings/WsMessage";
import { AUDIT_LIMIT, ConnectionProvider, reduceSnapshot } from "@/connection/ConnectionProvider";
import { useAudit, useFleet } from "@/connection/hooks";
import { host } from "./helpers";

const snap = (over: Partial<Snapshot> = {}): Snapshot => ({
  hosts: [host({ name: "desktop" })],
  channels: [
    { name: "unstable", head: "a".repeat(40), head_subject: "a" },
    { name: "stable", head: "b".repeat(40), head_subject: "b" },
  ],
  promote_candidate: null,
  promote: { from: "unstable", to: "stable" },
  viewer: "alice@example.com",
  demo: false,
  services: [],
  ...over,
});

describe("reduceSnapshot", () => {
  it("replaces on snapshot, ignores updates before the first one", () => {
    const s = snap();
    expect(reduceSnapshot(null, { type: "snapshot", data: s })).toBe(s);
    expect(reduceSnapshot(null, { type: "host", data: host({}) })).toBeNull();
    expect(reduceSnapshot(null, { type: "channels", data: [] })).toBeNull();
  });
  it("replaces a known host in place and appends a new one", () => {
    const s = snap({ hosts: [host({ name: "a" }), host({ name: "b" })] });
    const changed = reduceSnapshot(s, { type: "host", data: host({ name: "a", behind: 5 }) })!;
    expect(changed.hosts.map((h) => [h.name, h.behind])).toEqual([["a", 5], ["b", 0]]);
    const added = reduceSnapshot(s, { type: "host", data: host({ name: "c" }) })!;
    expect(added.hosts.map((h) => h.name)).toEqual(["a", "b", "c"]);
    expect(s.hosts).toHaveLength(2); // not mutated
  });
  it("replaces channels and the candidate, keeping the rest", () => {
    const s = snap();
    const ch: ChannelView[] = [{ name: "unstable", head: null, head_subject: null }];
    expect(reduceSnapshot(s, { type: "channels", data: ch })).toEqual({ ...s, channels: ch });
    expect(reduceSnapshot(s, { type: "candidate", data: "c".repeat(40) })).toEqual({
      ...s,
      promote_candidate: "c".repeat(40),
    });
  });
});

/** A WebSocket the test opens and feeds by hand. */
class FakeSocket {
  static all: FakeSocket[] = [];
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((e: { data: unknown }) => void) | null = null;
  onerror: (() => void) | null = null;
  constructor(readonly url: string) {
    FakeSocket.all.push(this);
  }
  close() {}
}

type Pending = { resolve: (s: Snapshot) => void };

describe("ConnectionProvider", () => {
  let snapshotRequests: Pending[];
  let auditRequests: string[];
  let seen: { snapshot: Snapshot | null; audit: AuditEvent[] | null };

  function Probe() {
    const { snapshot } = useFleet();
    const audit = useAudit();
    seen = { snapshot, audit };
    return null;
  }

  /** Lets pending fetches and their `.json()` settle (real setImmediate, fake timers). */
  async function flush() {
    await act(async () => {
      for (let i = 0; i < 5; i++) await new Promise((r) => setImmediate(r));
    });
  }

  async function send(m: WsMessage) {
    await act(async () => {
      FakeSocket.all[0].onmessage!({ data: JSON.stringify(m) });
    });
    await flush();
  }

  async function advance(ms: number) {
    await act(async () => {
      vi.advanceTimersByTime(ms);
    });
    await flush();
  }

  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
    FakeSocket.all = [];
    snapshotRequests = [];
    auditRequests = [];
    vi.stubGlobal("WebSocket", FakeSocket);
    vi.stubGlobal(
      "fetch",
      vi.fn((url: string) => {
        if (url.startsWith("/api/audit")) {
          auditRequests.push(url);
          return Promise.resolve(Response.json([]));
        }
        if (url === "/api/snapshot")
          return new Promise<Response>((resolve) =>
            snapshotRequests.push({ resolve: (s) => resolve(Response.json(s)) }),
          );
        return Promise.reject(new Error(`unexpected ${url}`));
      }),
    );
    render(
      <ConnectionProvider>
        <Probe />
      </ConnectionProvider>,
    );
    act(() => FakeSocket.all[0].onopen!());
  });
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  const heads = (u: string, s: string): ChannelView[] => [
    { name: "unstable", head: u.repeat(40), head_subject: null },
    { name: "stable", head: s.repeat(40), head_subject: null },
  ];

  it("asks for the candidate when heads change, and keeps only the newest answer", async () => {
    await send({ type: "snapshot", data: snap({ channels: heads("a", "b") }) });
    await send({ type: "channels_updated", data: heads("a", "b") });
    expect(snapshotRequests).toHaveLength(0); // same heads: nothing to ask
    await send({ type: "channels_updated", data: heads("c", "b") });
    await send({ type: "channels_updated", data: heads("d", "b") });
    expect(snapshotRequests).toHaveLength(2);
    // Answers arrive swapped: the newer one first, then the older one.
    snapshotRequests[1].resolve(snap({ promote_candidate: "d".repeat(40) }));
    await flush();
    snapshotRequests[0].resolve(snap({ promote_candidate: "c".repeat(40) }));
    await flush();
    expect(seen.snapshot!.promote_candidate).toBe("d".repeat(40));
    expect(seen.snapshot!.channels).toEqual(heads("d", "b"));
  });

  it("drops a candidate answer older than a snapshot", async () => {
    await send({ type: "snapshot", data: snap({ channels: heads("a", "b") }) });
    await send({ type: "channels_updated", data: heads("c", "b") });
    await send({ type: "snapshot", data: snap({ channels: heads("c", "b"), promote_candidate: "e".repeat(40) }) });
    snapshotRequests[0].resolve(snap({ promote_candidate: "c".repeat(40) }));
    await flush();
    expect(seen.snapshot!.promote_candidate).toBe("e".repeat(40));
  });

  it("loads the audit list on a snapshot, then once per burst of updates", async () => {
    await send({ type: "snapshot", data: snap() });
    expect(auditRequests).toEqual([`/api/audit?limit=${AUDIT_LIMIT}`]);
    expect(seen.audit).toEqual([]);
    for (let i = 0; i < 3; i++) {
      await send({ type: "host_updated", data: host({ name: "desktop", behind: i }) });
      await advance(300);
    }
    await send({ type: "channels_updated", data: heads("a", "b") });
    expect(auditRequests).toHaveLength(1); // still waiting for the burst to end
    await advance(1000);
    expect(auditRequests).toHaveLength(2);
    await advance(5000);
    expect(auditRequests).toHaveLength(2);
  });
});
