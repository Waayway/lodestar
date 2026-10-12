// The connection layer: the HTTP client (headers, errors, 403) and the WebSocket feed
// (reconnect with backoff, stopping on 403).
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ApiError, applyHost, getAudit, getSnapshot, isForbidden, promote } from "@/connection/client";
import { BACKOFF_START_MS, connectFeed, type FeedStatus, nextBackoff } from "@/connection/feed";

function stubFetch() {
  const fetchMock = vi.fn(async () => new Response("{}", { status: 200 }));
  vi.stubGlobal("fetch", fetchMock);
  return fetchMock;
}

function headersOf(call: unknown[]): Record<string, string> {
  return (call[1] as RequestInit).headers as Record<string, string>;
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("request headers", () => {
  it("sends X-Lodestar on every request (CSRF guard)", async () => {
    const fetchMock = stubFetch();
    await getSnapshot();
    await applyHost("desktop");
    await promote("a".repeat(40));
    expect(fetchMock).toHaveBeenCalledTimes(3);
    for (const call of fetchMock.mock.calls) {
      expect(headersOf(call)["x-lodestar"]).toBe("1");
    }
    expect(headersOf(fetchMock.mock.calls[0])["content-type"]).toBeUndefined();
    expect(headersOf(fetchMock.mock.calls[2])["content-type"]).toBe("application/json");
  });
});

describe("client errors", () => {
  it("carries the status and the server's {error} text", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => Response.json({ error: "commit is not on unstable" }, { status: 409 })));
    const e = await promote("a".repeat(40)).catch((e: unknown) => e);
    expect(e).toBeInstanceOf(ApiError);
    expect((e as ApiError).status).toBe(409);
    expect((e as ApiError).message).toBe("commit is not on unstable");
  });
  it("keeps the status line when the body is not JSON", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("<html>", { status: 502, statusText: "Bad Gateway" })));
    await expect(getSnapshot()).rejects.toMatchObject({ status: 502, message: "502 Bad Gateway" });
  });
  it("reports an unreachable server as status 0", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => {
      throw new TypeError("network");
    }));
    await expect(getSnapshot()).rejects.toMatchObject({ status: 0, message: "The Lodestar server is not answering" });
  });
  it("detects 403 (and only 403) as forbidden", async () => {
    vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 403 })));
    expect(await isForbidden()).toBe(true);
    vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 500 })));
    expect(await isForbidden()).toBe(false);
    vi.stubGlobal("fetch", vi.fn(async () => new Response("{}", { status: 200 })));
    expect(await isForbidden()).toBe(false);
  });
  it("asks for the audit list with a limit", async () => {
    const fetchMock = vi.fn(async (_url: string) => new Response("[]", { status: 200 }));
    vi.stubGlobal("fetch", fetchMock);
    expect(await getAudit(8)).toEqual([]);
    expect(fetchMock.mock.calls[0][0]).toBe("/api/audit?limit=8");
  });
});

describe("nextBackoff", () => {
  it("doubles from 1 s and stops at 30 s", () => {
    const seq = [BACKOFF_START_MS];
    while (seq.length < 7) seq.push(nextBackoff(seq[seq.length - 1]));
    expect(seq).toEqual([1000, 2000, 4000, 8000, 16000, 30000, 30000]);
  });
});

/** A WebSocket that never connects: each instance is closed by the test. */
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

describe("connectFeed access check", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    FakeSocket.all = [];
    vi.stubGlobal("WebSocket", FakeSocket);
    vi.stubGlobal("location", { protocol: "http:", host: "lodestar.test" });
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.unstubAllGlobals();
  });

  async function firstCloseWith(forbidden: boolean) {
    const statuses: FeedStatus["kind"][] = [];
    const isForbidden = vi.fn(async () => forbidden);
    const stop = connectFeed({ onMessage: () => {}, onStatus: (s) => statuses.push(s.kind), isForbidden });
    await FakeSocket.all[0].onclose!();
    await vi.advanceTimersByTimeAsync(BACKOFF_START_MS);
    return { statuses, isForbidden, stop };
  }

  it("stops reconnecting when the viewer is forbidden", async () => {
    const { statuses, isForbidden } = await firstCloseWith(true);
    expect(isForbidden).toHaveBeenCalledTimes(1);
    expect(statuses).toEqual(["connecting", "forbidden"]);
    expect(FakeSocket.all).toHaveLength(1);
  });

  it("asks only once, then keeps retrying with backoff", async () => {
    const { statuses, isForbidden, stop } = await firstCloseWith(false);
    expect(statuses).toEqual(["connecting", "waiting", "connecting"]);
    expect(FakeSocket.all).toHaveLength(2);
    await FakeSocket.all[1].onclose!();
    expect(isForbidden).toHaveBeenCalledTimes(1);
    stop();
  });
});
