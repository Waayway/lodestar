// REST calls to the Lodestar server. Errors carry the HTTP status and the server's `{error}` text.
// The only module (with `feed.ts`) that talks to the network: components go through
// `connection/hooks`.
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { BuildsView } from "@/bindings/BuildsView";
import type { Me } from "@/bindings/Me";
import type { PublicService } from "@/bindings/PublicService";
import type { Snapshot } from "@/bindings/Snapshot";
import type { UpdatesView } from "@/bindings/UpdatesView";

import type { ApplyChannelResponse, ApplyHostResponse } from "@/lib/apiTypes";

export type { AgentStatus, ApplyChannelResponse, ApplyHostResponse } from "@/lib/apiTypes";

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

/** Tracked software with its latest release, the updater's inputs, and whether a run is going. */
export async function getUpdates(): Promise<UpdatesView> {
  return (await request("GET", "/api/updates")).json();
}

/** Starts an updater run ("Check now"); a 409 ApiError means one is already running. */
export async function runUpdates(): Promise<void> {
  await request("POST", "/api/updates/run");
}

/** Whether the build gate is on and the newest builds, every host with its path or log. */
export async function getRecentBuilds(): Promise<BuildsView> {
  return (await request("GET", "/api/builds/recent")).json();
}

/** Removes the failed hosts from a commit's build result so they are built again. */
export async function retryBuild(commit: string): Promise<{ retried: string[] }> {
  return (await request("POST", `/api/builds/${enc(commit)}/retry`)).json();
}

export async function getApplyLog(host: string, applyId: string): Promise<string> {
  return (await request("GET", `/api/hosts/${enc(host)}/applies/${enc(applyId)}/log`)).text();
}

/** The newest audit events, newest first. */
export async function getAudit(limit: number): Promise<AuditEvent[]> {
  return (await request("GET", `/api/audit?limit=${limit}`)).json();
}

/** Who the server thinks the viewer is. */
export async function getMe(): Promise<Me> {
  return (await request("GET", "/api/me")).json();
}

/** The reduced service list an anonymous visitor may see. */
export async function getPublicServices(): Promise<PublicService[]> {
  return (await request("GET", "/api/public/services")).json();
}

/** Ends the session (the server clears the cookie; answers 204). */
export async function logout(): Promise<void> {
  await request("POST", "/auth/logout");
}

/** Where the login starts; `next` is the page to come back to. */
export function loginUrl(next: string): string {
  return `/auth/login?next=${enc(next)}`;
}
