// The dashboard's data: the snapshot kept current by the feed, the open apply log, the audit
// list, and the actions that change things on the server. Components read it through
// `connection/hooks`; nothing else calls `fetch` or opens a socket.
import { createContext, type ComponentChildren } from "preact";
import { useCallback, useEffect, useMemo, useReducer, useRef, useState } from "preact/hooks";
import { toast } from "sonner";
import type { AuditEvent } from "@/bindings/AuditEvent";
import type { ChannelView } from "@/bindings/ChannelView";
import type { HostView } from "@/bindings/HostView";
import type { LogChunk } from "@/bindings/LogChunk";
import type { Me } from "@/bindings/Me";
import type { PromoteChannels } from "@/bindings/PromoteChannels";
import type { PublicService } from "@/bindings/PublicService";
import type { ServiceView } from "@/bindings/ServiceView";
import type { Snapshot } from "@/bindings/Snapshot";
import type { WsMessage } from "@/bindings/WsMessage";
import * as client from "@/connection/client";
import { BACKOFF_START_MS, connectFeed, type FeedStatus, nextBackoff } from "@/connection/feed";
import {
  type FetchState,
  finishFetch,
  fromText,
  type HeldLog,
  type LogWatch,
  mergeChunk,
  requestFetch,
  watchLog,
} from "@/lib/applyLog";
import { agentStatusText, shortRev } from "@/lib/format";
import { PUBLIC_POLL_MS, type ViewMode, viewMode } from "@/lib/viewer";

type Action =
  | { type: "snapshot"; data: Snapshot }
  | { type: "host"; data: HostView }
  | { type: "channels"; data: ChannelView[] }
  | { type: "candidate"; data: string | null }
  | { type: "services"; data: ServiceView[] };

export function reduceSnapshot(s: Snapshot | null, a: Action): Snapshot | null {
  if (a.type === "snapshot") return a.data;
  if (s === null) return s;
  switch (a.type) {
    case "host": {
      const i = s.hosts.findIndex((h) => h.name === a.data.name);
      const hosts = i < 0 ? [...s.hosts, a.data] : s.hosts.map((h, j) => (j === i ? a.data : h));
      return { ...s, hosts };
    }
    case "channels":
      return { ...s, channels: a.data };
    case "candidate":
      return { ...s, promote_candidate: a.data };
    case "services":
      return { ...s, services: a.data };
  }
}

const headsKey = (channels: ChannelView[]) =>
  channels.map((c) => `${c.name}=${c.head ?? ""}`).join(" ");

/** The apply whose log is open in the sheet, and what we hold of that log. */
export type OpenLog = LogWatch & FetchState & { log: HeldLog | null; error: string | null };

/** How many audit events the dashboard shows. */
export const AUDIT_LIMIT = 8;
/** Feed messages that may add audit rows are batched into one request this long after. */
const AUDIT_DEBOUNCE_MS = 1000;

export type Actions = {
  /** Sends "apply newest" to one host; toasts the outcome. */
  applyHost: (host: string) => Promise<void>;
  /** Sends "apply newest" to every host on a channel; toasts the outcome. */
  applyChannel: (channel: string) => Promise<void>;
  /** Promotes `commit` along `pair`. True when it was pushed (the dialog closes then). */
  promote: (commit: string, pair: PromoteChannels) => Promise<boolean>;
  /** Opens the apply log sheet on one apply and fetches its log. */
  showLog: (host: string, applyId: string) => void;
  closeLog: () => void;
  /** Fetches the open log again (one request at a time). */
  fetchLog: () => void;
  /** Goes to the login and comes back to this page. */
  login: () => void;
  /** Ends the session, then goes to the start page. */
  logout: () => Promise<void>;
};

export type ConnectionState = {
  snapshot: Snapshot | null;
  feed: FeedStatus;
  openLog: OpenLog | null;
  /** Newest first; null until the first answer. */
  audit: AuditEvent[] | null;
  /** Who the server says the viewer is; null until the first answer. */
  viewer: Me | null;
  mode: ViewMode;
  /** What an anonymous visitor sees; null until the first answer. */
  publicServices: PublicService[] | null;
};

export const StateContext = createContext<ConnectionState | null>(null);
export const ActionsContext = createContext<Actions | null>(null);

function reportError(what: string, e: unknown) {
  toast.error(what, { description: (e as Error).message });
}

export function ConnectionProvider({ children }: { children: ComponentChildren }) {
  const [snap, dispatch] = useReducer(reduceSnapshot, null);
  const [feed, setFeed] = useState<FeedStatus>({ kind: "connecting" });
  const [audit, setAudit] = useState<AuditEvent[] | null>(null);
  // The open log lives in a ref so WebSocket chunks and the log request always merge onto the
  // latest copy; the state mirrors it for rendering.
  const [openLog, setOpenLogState] = useState<OpenLog | null>(null);
  const openLogRef = useRef<OpenLog | null>(null);
  const setOpenLog = useCallback((o: OpenLog | null) => {
    openLogRef.current = o;
    setOpenLogState(o);
  }, []);
  const snapRef = useRef<Snapshot | null>(null);
  snapRef.current = snap;
  const [viewer, setViewer] = useState<Me | null>(null);
  const [meForbidden, setMeForbidden] = useState(false);
  const [publicServices, setPublicServices] = useState<PublicService[] | null>(null);
  const candidateSeq = useRef(0);
  const heads = useRef("");

  // Fetches the open apply's whole log, one request at a time: asking while one is in flight
  // only marks it to run once more when the current one finishes (see `requestFetch`).
  const fetchLog = useCallback(() => {
    const o = openLogRef.current;
    if (!o) return;
    const r = requestFetch(o);
    setOpenLog({ ...o, ...r.next });
    if (r.start) void runFetches(o.host, o.applyId);

    async function runFetches(host: string, applyId: string) {
      for (;;) {
        let result: Partial<OpenLog>;
        try {
          const fetched = fromText(await client.getApplyLog(host, applyId));
          const held = openLogRef.current?.log;
          // Chunks that arrived while the request was in flight may already be past it.
          result = { log: held && held.bytes > fetched.bytes ? held : fetched, error: null };
        } catch (e) {
          result = { error: (e as Error).message };
        }
        const cur = openLogRef.current;
        if (!cur || cur.applyId !== applyId) return;
        const f = finishFetch(cur);
        setOpenLog({ ...cur, ...result, ...f.next });
        if (!f.start) return;
      }
    }
  }, [setOpenLog]);

  const onChunk = useCallback(
    (c: LogChunk) => {
      const o = openLogRef.current;
      if (!o || o.applyId !== c.apply_id) return;
      const m = mergeChunk(o.log ?? undefined, c);
      if (m.kind === "appended") setOpenLog({ ...o, log: m.log });
      else if (m.kind === "gap") fetchLog();
    },
    [fetchLog, setOpenLog],
  );

  // The audit list: refreshed shortly after anything that can add a row (a snapshot, a host
  // or channel update), one request at a time, failures keep the last list.
  const auditTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const auditFetch = useRef<FetchState>({ fetching: false, again: false });
  const loadAudit = useCallback(() => {
    const r = requestFetch(auditFetch.current);
    auditFetch.current = r.next;
    if (!r.start) return;
    void (async () => {
      for (;;) {
        try {
          setAudit(await client.getAudit(AUDIT_LIMIT));
        } catch {
          // Keep what is shown; the next update tries again.
        }
        const f = finishFetch(auditFetch.current);
        auditFetch.current = f.next;
        if (!f.start) return;
      }
    })();
  }, []);
  const refreshAudit = useCallback(() => {
    if (auditTimer.current) clearTimeout(auditTimer.current);
    auditTimer.current = setTimeout(loadAudit, AUDIT_DEBOUNCE_MS);
  }, [loadAudit]);
  useEffect(
    () => () => {
      if (auditTimer.current) clearTimeout(auditTimer.current);
    },
    [],
  );

  // Who is looking: asked once on mount (a network error retries with the feed's backoff) and
  // again when the feed gives up (`waiting`) or an action answers 401, since the session may
  // have ended. A failed re-ask keeps what is known.
  const meSeq = useRef(0);
  const meTimer = useRef<ReturnType<typeof setTimeout> | null>(null);
  const meBackoff = useRef(BACKOFF_START_MS);
  const loadMe = useCallback(() => {
    const seq = ++meSeq.current;
    if (meTimer.current) clearTimeout(meTimer.current);
    client.getMe().then(
      (m) => {
        if (seq !== meSeq.current) return;
        meBackoff.current = BACKOFF_START_MS;
        setMeForbidden(false);
        setViewer(m);
      },
      (e: unknown) => {
        if (seq !== meSeq.current) return;
        if (e instanceof client.ApiError && e.status === 403) {
          setMeForbidden(true);
          return;
        }
        const wait = meBackoff.current;
        meBackoff.current = nextBackoff(wait);
        meTimer.current = setTimeout(loadMe, wait);
      },
    );
  }, []);
  useEffect(() => {
    loadMe();
    return () => {
      meSeq.current++;
      if (meTimer.current) clearTimeout(meTimer.current);
    };
  }, [loadMe]);

  const mode = viewMode(viewer, meForbidden || feed.kind === "forbidden");
  const signedIn = mode === "signed_in";
  const anonymous = mode === "anonymous";

  useEffect(() => {
    if (feed.kind === "waiting") loadMe();
  }, [feed.kind, loadMe]);

  // Anonymous visitors get the public list by polling; there is no socket for them.
  useEffect(() => {
    if (!anonymous) return;
    let stopped = false;
    const load = () =>
      client.getPublicServices().then(
        (l) => {
          if (!stopped) setPublicServices(l);
        },
        (e: unknown) => {
          // A session may have started elsewhere; otherwise keep the last list.
          if (!stopped && e instanceof client.ApiError && e.status === 401) loadMe();
        },
      );
    void load();
    const timer = setInterval(load, PUBLIC_POLL_MS);
    return () => {
      stopped = true;
      clearInterval(timer);
    };
  }, [anonymous, loadMe]);

  useEffect(
    () => {
      if (!signedIn) return;
      return connectFeed({
        onStatus: setFeed,
        isForbidden: client.isForbidden,
        onMessage: (m: WsMessage) => {
          const o = openLogRef.current;
          if (o) {
            const w = watchLog(o, m);
            if (w.running !== o.running) setOpenLog({ ...o, running: w.running });
            if (w.refetch) fetchLog();
          }
          switch (m.type) {
            case "host_updated":
              dispatch({ type: "host", data: m.data });
              refreshAudit();
              break;
            case "snapshot":
              heads.current = headsKey(m.data.channels);
              candidateSeq.current++; // a pending candidate answer is older than this
              dispatch({ type: "snapshot", data: m.data });
              loadAudit();
              break;
            case "channels_updated": {
              dispatch({ type: "channels", data: m.data });
              refreshAudit();
              // The promote candidate isn't part of this message; it changes only with the
              // heads. Only the newest request's answer counts, in case answers arrive swapped.
              const key = headsKey(m.data);
              if (key !== heads.current) {
                heads.current = key;
                const seq = ++candidateSeq.current;
                client.getSnapshot().then(
                  (s) => {
                    if (seq === candidateSeq.current)
                      dispatch({ type: "candidate", data: s.promote_candidate });
                  },
                  () => {},
                );
              }
              break;
            }
            case "services_updated":
              dispatch({ type: "services", data: m.data });
              break;
            case "apply_log":
              onChunk(m.data);
              break;
          }
        },
      });
    },
    [signedIn, onChunk, fetchLog, setOpenLog, loadAudit, refreshAudit],
  );

  // An action that answers 401 means the session is gone: re-ask who we are, then report.
  const fail = useCallback(
    (what: string, e: unknown) => {
      if (e instanceof client.ApiError && e.status === 401) loadMe();
      reportError(what, e);
    },
    [loadMe],
  );

  const actions = useMemo<Actions>(
    () => ({
      async applyHost(host) {
        try {
          const r = await client.applyHost(host);
          const note = agentStatusText(host, r.agent_status);
          if (note) toast.warning(note);
          else toast.success(`Apply started on ${host}`);
        } catch (e) {
          fail(`Apply on ${host} failed`, e);
        }
      },
      async applyChannel(channel) {
        try {
          const r = await client.applyChannel(channel);
          const notes = Object.entries(r.agent_status)
            .map(([host, s]) => agentStatusText(host, s))
            .filter((n): n is string => n !== null);
          const started = Object.values(r.agent_status).filter((s) => s === "202").length;
          const total = Object.keys(r.apply_ids).length;
          const summary = `Apply started on ${started} of ${total} ${total === 1 ? "machine" : "machines"} on ${channel}`;
          if (notes.length > 0)
            toast.warning(summary, {
              description: (
                <ul className="flex flex-col gap-1">
                  {notes.map((n) => (
                    <li key={n}>{n}</li>
                  ))}
                </ul>
              ),
            });
          else toast.success(summary);
        } catch (e) {
          fail(`Apply to ${channel} failed`, e);
        }
      },
      async promote(commit, pair) {
        try {
          await client.promote(commit);
          toast.success(`${pair.to} moved to ${shortRev(commit)}`);
          return true;
        } catch (e) {
          fail("Promote failed", e);
          return false;
        }
      },
      showLog(host, applyId) {
        const rec = snapRef.current?.hosts.find((h) => h.name === host)?.last_apply;
        const running = rec?.apply_id === applyId ? rec.state === "running" : null;
        setOpenLog({ host, applyId, running, log: null, error: null, fetching: false, again: false });
        fetchLog();
      },
      closeLog() {
        setOpenLog(null);
      },
      fetchLog,
      login() {
        location.assign(client.loginUrl(location.pathname + location.hash));
      },
      async logout() {
        try {
          await client.logout();
        } catch (e) {
          reportError("Logout failed", e);
          return;
        }
        location.assign("/");
      },
    }),
    [fetchLog, setOpenLog, fail],
  );

  const state = useMemo<ConnectionState>(
    () => ({ snapshot: snap, feed, openLog, audit, viewer, mode, publicServices }),
    [snap, feed, openLog, audit, viewer, mode, publicServices],
  );

  return (
    <ActionsContext.Provider value={actions}>
      <StateContext.Provider value={state}>{children}</StateContext.Provider>
    </ActionsContext.Provider>
  );
}
