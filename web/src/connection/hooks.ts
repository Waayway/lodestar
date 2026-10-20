// How components read the dashboard's data and act on it. The only way in: no component
// calls `fetch` or opens a socket itself.
import { useContext, useMemo } from "preact/hooks";
import type { ActivationEvent } from "@/bindings/ActivationEvent";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { Me } from "@/bindings/Me";
import type { PublicService } from "@/bindings/PublicService";
import type { ServiceView } from "@/bindings/ServiceView";
import type { Snapshot } from "@/bindings/Snapshot";
import {
  type Actions,
  ActionsContext,
  type ConnectionState,
  StateContext,
} from "@/connection/ConnectionProvider";
import type { FeedStatus } from "@/connection/feed";
import type { HeldLog } from "@/lib/applyLog";
import { actPermission, type ViewMode } from "@/lib/viewer";

function useConnection(): ConnectionState {
  const s = useContext(StateContext);
  if (s === null) throw new Error("useConnection outside <ConnectionProvider>");
  return s;
}

/** The whole fleet (null until the first snapshot) and the feed's status. */
export function useFleet(): { snapshot: Snapshot | null; feed: FeedStatus } {
  const { snapshot, feed } = useConnection();
  return { snapshot, feed };
}

export type ApplyLogView = {
  host: string;
  applyId: string;
  /** The host's last apply when it is this one, so its state stays live; else null. */
  record: ApplyRecord | null;
  log: HeldLog | null;
  error: string | null;
  /** The machine's last activation (when its system last switched), or null. */
  activation: ActivationEvent | null;
};

/**
 * The apply log open in the sheet, or null when none is. With `host` and `applyId`, only that
 * apply's log (null when another one is open).
 */
export function useApplyLog(host?: string, applyId?: string): ApplyLogView | null {
  const { snapshot, openLog } = useConnection();
  if (openLog === null) return null;
  if (host !== undefined && (openLog.host !== host || openLog.applyId !== applyId)) return null;
  const h = snapshot?.hosts.find((x) => x.name === openLog.host);
  const last = h?.last_apply ?? null;
  return {
    host: openLog.host,
    applyId: openLog.applyId,
    record: last?.apply_id === openLog.applyId ? last : null,
    log: openLog.log,
    error: openLog.error,
    activation: h?.last_activation ?? null,
  };
}

/** The newest audit events, newest first; null until loaded. */
export function useAudit(): AuditEvent[] | null {
  return useConnection().audit;
}

/**
 * The service links (null until the first snapshot), each with its host's current online state
 * from the fleet list, so a host update alone moves the cards. Links on a host outside the
 * fleet keep the state the server sent.
 */
export function useServices(): ServiceView[] | null {
  const { snapshot } = useConnection();
  return useMemo(() => {
    if (snapshot === null) return null;
    const online = new Map(snapshot.hosts.map((h) => [h.name, h.online]));
    return snapshot.services.map((s) => ({ ...s, host_online: online.get(s.host) ?? s.host_online }));
  }, [snapshot]);
}

export function useActions(): Actions {
  const a = useContext(ActionsContext);
  if (a === null) throw new Error("useActions outside <ConnectionProvider>");
  return a;
}

/** Who the server says the viewer is; null until the first answer. */
export function useViewer(): Me | null {
  return useConnection().viewer;
}

/** Which state to render: loading, anonymous, signed in, or refused. */
export function useViewMode(): ViewMode {
  return useConnection().mode;
}

/** When `/api/me` is asked again after it could not be reached; null while it answers. */
export function useMeRetryAt(): number | null {
  return useConnection().meRetryAt;
}

/** The public service list for anonymous visitors; null until the first answer. */
export function usePublicServices(): PublicService[] | null {
  return useConnection().publicServices;
}

/** Whether action buttons show, and why they are disabled. */
export function useActPermission(): { visible: boolean; disabledReason: string | null } {
  const { viewer } = useConnection();
  return useMemo(() => actPermission(viewer), [viewer]);
}
