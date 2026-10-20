// Fixtures and a fake ActionsContext shared by the component tests.
import type { ComponentChildren } from "preact";
import { vi } from "vitest";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { HostView } from "@/bindings/HostView";
import type { Me } from "@/bindings/Me";
import type { PublicService } from "@/bindings/PublicService";
import type { ServiceView } from "@/bindings/ServiceView";
import { TooltipProvider } from "@/components/ui/tooltip";
import { type Actions, ActionsContext, type ConnectionState, StateContext } from "@/connection/ConnectionProvider";

export const REV = "0123456789abcdef0123456789abcdef01234567";
export const NOW = 10_000;

export function apply(over: Partial<ApplyRecord>): ApplyRecord {
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

export function host(over: Partial<HostView>): HostView {
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

export function service(over: Partial<ServiceView>): ServiceView {
  return {
    id: "grafana",
    title: "Grafana",
    url: "https://grafana.example.test",
    group: "Monitoring",
    description: null,
    icon: null,
    host: "desktop",
    host_online: "on",
    check: { state: "up", reason: null, latency_ms: 12, checked_at: NOW - 5, since: NOW - 100 },
    ...over,
  };
}

export function me(over: Partial<Me>): Me {
  return { viewer: "bob", role: "admin", session: true, can_act: true, reason: null, ...over };
}

export function publicService(over: Partial<PublicService>): PublicService {
  return {
    title: "Grafana",
    group: "Monitoring",
    description: null,
    icon: null,
    state: "up",
    since: NOW - 100,
    ...over,
  };
}

export function fakeActions(): Actions & { [K in keyof Actions]: ReturnType<typeof vi.fn> } {
  return {
    applyHost: vi.fn(async () => {}),
    applyChannel: vi.fn(async () => {}),
    promote: vi.fn(async () => true),
    showLog: vi.fn(),
    closeLog: vi.fn(),
    fetchLog: vi.fn(),
    login: vi.fn(),
    logout: vi.fn(async () => {}),
  } as never;
}

/** A connection state for components that read the viewer; defaults to a tailnet admin. */
export function connectionState(over: Partial<ConnectionState> = {}): ConnectionState {
  return {
    snapshot: null,
    feed: { kind: "live" } as never,
    openLog: null,
    audit: null,
    viewer: me({}),
    mode: "signed_in",
    meRetryAt: null,
    publicServices: null,
    ...over,
  };
}

export function withActions(actions: Actions, children: ComponentChildren, state: Partial<ConnectionState> = {}) {
  return (
    <StateContext.Provider value={connectionState(state)}>
      <ActionsContext.Provider value={actions}>
        <TooltipProvider>{children}</TooltipProvider>
      </ActionsContext.Provider>
    </StateContext.Provider>
  );
}


/** The Lucide icon in an element, by its `lucide-<name>` class. */
export const iconOf = (el: Element) =>
  [...(el.querySelector("svg")?.classList ?? [])].find((c) => c.startsWith("lucide-") && c !== "lucide-icon");

