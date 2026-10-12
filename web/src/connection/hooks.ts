// How components read the dashboard's data and act on it. The only way in: no component
// calls `fetch` or opens a socket itself.
import { useContext } from "preact/hooks";
import type { ActivationEvent } from "@/bindings/ActivationEvent";
import type { ApplyRecord } from "@/bindings/ApplyRecord";
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { Snapshot } from "@/bindings/Snapshot";
import {
  type Actions,
  ActionsContext,
  type ConnectionState,
  StateContext,
} from "@/connection/ConnectionProvider";
import type { FeedStatus } from "@/connection/feed";
import type { HeldLog } from "@/lib/applyLog";

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

export function useActions(): Actions {
  const a = useContext(ActionsContext);
  if (a === null) throw new Error("useActions outside <ConnectionProvider>");
  return a;
}
