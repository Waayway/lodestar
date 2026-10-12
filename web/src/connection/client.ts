// REST calls to the Lodestar server. Errors carry the HTTP status and the server's `{error}` text.
// The only module (with `feed.ts`) that talks to the network: components go through
// `connection/hooks`.
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { Snapshot } from "@/bindings/Snapshot";

/** What the server got back from the agent when it sent an apply. */
export type AgentStatus = "202" | "409" | "200" | "unreachable" | "error";

export type ApplyHostResponse = { apply_id: string; agent_status: AgentStatus };
export type ApplyChannelResponse = {
  apply_ids: Record<string, string>;
  agent_status: Record<string, AgentStatus>;
};

export class ApiError extends Error {
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

async function request(method: string, path: string, body?: unknown): Promise<Response> {
  // X-Lodestar: the server refuses state-changing requests without it (CSRF guard).
  const headers: Record<string, string> = { "x-lodestar": "1" };
  if (body !== undefined) headers["content-type"] = "application/json";
  let res: Response;
  try {
    res = await fetch(path, {
      method,
      headers,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
  } catch {
    throw new ApiError(0, "The Lodestar server is not answering");
  }
  if (!res.ok) {
    let message = `${res.status} ${res.statusText}`.trim();
    try {
      const j = (await res.json()) as { error?: unknown };
      if (typeof j.error === "string" && j.error) message = j.error;
    } catch {
      // Not JSON: keep the status line.
    }
    throw new ApiError(res.status, message);
  }
  return res;
}

const enc = encodeURIComponent;

/** True when the server answers this viewer with 403 (not an allowed login or tag). */
export async function isForbidden(): Promise<boolean> {
  try {
    await request("GET", "/api/snapshot");
    return false;
  } catch (e) {
    return e instanceof ApiError && e.status === 403;
  }
}

export async function getSnapshot(): Promise<Snapshot> {
  return (await request("GET", "/api/snapshot")).json();
}

export async function applyHost(host: string): Promise<ApplyHostResponse> {
  return (await request("POST", `/api/hosts/${enc(host)}/apply`)).json();
}

export async function applyChannel(channel: string): Promise<ApplyChannelResponse> {
  return (await request("POST", `/api/channels/${enc(channel)}/apply`)).json();
}

export async function promote(commit: string): Promise<void> {
  await request("POST", "/api/promote", { commit });
}

export async function getApplyLog(host: string, applyId: string): Promise<string> {
  return (await request("GET", `/api/hosts/${enc(host)}/applies/${enc(applyId)}/log`)).text();
}

/** The newest audit events, newest first. */
export async function getAudit(limit: number): Promise<AuditEvent[]> {
  return (await request("GET", `/api/audit?limit=${limit}`)).json();
}
