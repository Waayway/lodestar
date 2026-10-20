// @vitest-environment jsdom
// The viewer: `/api/me` decides what the dashboard renders. Pure tables, the client calls, and
// the provider's flow (public feed polling, feed only when signed in, re-asking on `waiting`).
import { act, cleanup, render } from "@testing-library/preact";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { getMe, getPublicServices, loginUrl, logout } from "@/connection/client";
import { ConnectionProvider } from "@/connection/ConnectionProvider";
import { useActPermission, useViewMode, useViewer, usePublicServices } from "@/connection/hooks";
import { actPermission, PUBLIC_POLL_MS, viewMode } from "@/lib/viewer";
import { me, publicService } from "./helpers";

describe("viewMode", () => {
  it("maps the answer to a state", () => {
    expect(viewMode(null, false)).toBe("loading");
    expect(viewMode(me({ role: "anonymous" }), false)).toBe("anonymous");
    expect(viewMode(me({ role: "reader" }), false)).toBe("signed_in");
    expect(viewMode(me({ role: "admin" }), false)).toBe("signed_in");
    expect(viewMode(null, true)).toBe("forbidden");
    expect(viewMode(me({ role: "admin" }), true)).toBe("forbidden");
  });
});

describe("actPermission", () => {
  it("shows actions to admins only, disabled without the tailnet mark", () => {
    const hidden = { visible: false, disabledReason: null };
    expect(actPermission(null)).toEqual(hidden);
    expect(actPermission(me({ role: "anonymous" }))).toEqual(hidden);
    expect(actPermission(me({ role: "reader" }))).toEqual(hidden);
    expect(actPermission(me({ role: "admin", can_act: false, reason: "not_tailnet" }))).toEqual({
      visible: true,
      disabledReason: "Only over Tailscale",
    });
    expect(actPermission(me({ role: "admin", can_act: true }))).toEqual({
      visible: true,
      disabledReason: null,
    });
  });
});

describe("client", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("builds the login URL with the page to come back to", () => {
    expect(loginUrl("/#/services")).toBe("/auth/login?next=%2F%23%2Fservices");
  });
  it("posts the logout with X-Lodestar", async () => {
    const fetchMock = vi.fn(async (_u: string, _i?: RequestInit) => new Response(null, { status: 204 }));
    vi.stubGlobal("fetch", fetchMock);
    await logout();
    expect(fetchMock.mock.calls[0][0]).toBe("/auth/logout");
    const init = fetchMock.mock.calls[0][1] as RequestInit;
    expect(init.method).toBe("POST");
    expect((init.headers as Record<string, string>)["x-lodestar"]).toBe("1");
  });
  it("reads /api/me and /api/public/services", async () => {
    const fetchMock = vi.fn(async (u: string) =>
      u === "/api/me" ? Response.json(me({})) : Response.json([publicService({})]),
    );
    vi.stubGlobal("fetch", fetchMock);
    expect(await getMe()).toEqual(me({}));
    expect(await getPublicServices()).toEqual([publicService({})]);
  });
});

class CountingSocket {
  static count = 0;
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: unknown = null;
  onerror: unknown = null;
  constructor() {
    CountingSocket.count++;
  }
  close() {}
}

type Seen = { mode?: string; viewer?: unknown; services?: unknown; permission?: unknown };

function Probe({ seen }: { seen: Seen }) {
  seen.mode = useViewMode();
  seen.viewer = useViewer();
  seen.services = usePublicServices();
  seen.permission = useActPermission();
  return null;
}

/** Stubs fetch by path; `answers.me` may change between calls. */
function stubApi(answers: { me: () => Response; calls: string[] }) {
  vi.stubGlobal(
    "fetch",
    vi.fn(async (u: string) => {
      answers.calls.push(u);
      if (u === "/api/me") return answers.me();
      if (u === "/api/public/services") return Response.json([publicService({})]);
      return Response.json({ hosts: [], channels: [], services: [], promote_candidate: null });
    }),
  );
}

/** Lets pending fetches and their `.json()` settle (real setImmediate, fake timers). */
async function flush() {
  await act(async () => {
    for (let i = 0; i < 20; i++) {
      await new Promise((r) => setImmediate(r));
      vi.advanceTimersByTime(0); // Preact schedules renders with setTimeout
    }
  });
}

async function advance(ms: number) {
  await act(async () => {
    vi.advanceTimersByTime(ms);
  });
  await flush();
}

async function mount(seen: Seen) {
  render(
    <ConnectionProvider>
      <Probe seen={seen} />
    </ConnectionProvider>,
  );
  await flush();
  await flush(); // a state change starts the next fetch; settle that too
}

describe("ConnectionProvider viewer flow", () => {
  afterEach(cleanup);
  beforeEach(() => {
    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "setInterval", "clearInterval", "Date"] });
    CountingSocket.count = 0;
    vi.stubGlobal("WebSocket", CountingSocket);
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  it("anonymous: fetches the public feed now and every 30 s, opens no socket", async () => {
    const calls: string[] = [];
    stubApi({ me: () => Response.json(me({ role: "anonymous", viewer: null, can_act: false, reason: "anonymous" })), calls });
    const seen: Seen = {};
    await mount(seen);
    expect(seen.mode).toBe("anonymous");
    expect(calls.filter((c) => c === "/api/public/services")).toHaveLength(1);
    expect(seen.services).toEqual([publicService({})]);
    await advance(PUBLIC_POLL_MS);
    expect(calls.filter((c) => c === "/api/public/services")).toHaveLength(2);
    expect(CountingSocket.count).toBe(0);
  });

  it("reader: one socket, no public fetch", async () => {
    const calls: string[] = [];
    stubApi({ me: () => Response.json(me({ role: "reader", can_act: false, reason: "not_admin" })), calls });
    const seen: Seen = {};
    await mount(seen);
    expect(seen.mode).toBe("signed_in");
    expect(CountingSocket.count).toBe(1);
    expect(calls).not.toContain("/api/public/services");
  });

  it("a feed that goes waiting re-asks /api/me; anonymous then starts the public feed", async () => {
    const calls: string[] = [];
    let current = me({ role: "reader", can_act: false, reason: "not_admin" });
    stubApi({ me: () => Response.json(current), calls });
    const sockets: CountingSocket[] = [];
    vi.stubGlobal(
      "WebSocket",
      class extends CountingSocket {
        constructor() {
          super();
          sockets.push(this);
        }
      },
    );
    const seen: Seen = {};
    await mount(seen);
    expect(seen.mode).toBe("signed_in");
    current = me({ role: "anonymous", viewer: null, can_act: false, reason: "anonymous" });
    await act(async () => {
      sockets[0].onclose?.();
    });
    await flush();
    await flush();
    expect(seen.mode).toBe("anonymous");
    expect(calls).toContain("/api/public/services");
  });

  it("403 from /api/me is the forbidden state", async () => {
    const calls: string[] = [];
    stubApi({ me: () => new Response("", { status: 403 }), calls });
    const seen: Seen = {};
    await mount(seen);
    expect(seen.mode).toBe("forbidden");
    expect(CountingSocket.count).toBe(0);
  });

  it("retries /api/me after a network error", async () => {
    const calls: string[] = [];
    let n = 0;
    stubApi({
      me: () => {
        if (n++ === 0) throw new TypeError("network");
        return Response.json(me({ role: "reader" }));
      },
      calls,
    });
    const seen: Seen = {};
    await mount(seen);
    expect(seen.mode).toBe("loading");
    await advance(1000);
    expect(seen.mode).toBe("signed_in");
  });
});
