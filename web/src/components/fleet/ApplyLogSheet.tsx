import { useLayoutEffect, useRef } from "preact/hooks";
import { CommitHash } from "@/components/fleet/CommitHash";
import { StatusBadge } from "@/components/fleet/StatusBadge";
import {
  Sheet,
  SheetContent,
  SheetDescription,
  SheetHeader,
  SheetTitle,
} from "@/components/ui/sheet";
import { useActions, useApplyLog } from "@/connection/hooks";
import { Building, Activation as TerminalIcon } from "@/icons";
import { ago, durationLabel } from "@/lib/time";

/** The log of the apply opened from a row, appended live while it runs. */
export function ApplyLogSheet({ now }: { now: number }) {
  const view = useApplyLog();
  const { closeLog } = useActions();
  const scroller = useRef<HTMLPreElement>(null);
  const pinned = useRef(true);
  const open = view !== null;
  const record = view?.record ?? null;
  const log = view?.log ?? null;
  const error = view?.error ?? null;
  const running = record?.state === "running";
  if (!open) pinned.current = true;

  // Follow the end of the log unless the reader scrolled up.
  useLayoutEffect(() => {
    const el = scroller.current;
    if (el && pinned.current) el.scrollTop = el.scrollHeight;
  }, [log?.text, open]);

  return (
    <Sheet open={open} onOpenChange={(o) => !o && closeLog()}>
      <SheetContent side="right" className="w-full gap-0 sm:max-w-3xl">
        <SheetHeader className="border-line border-b pr-12">
          <SheetTitle className="flex items-center gap-2">
            <TerminalIcon aria-hidden="true" className="text-accent size-4" />
            <span>{view?.host ?? ""}</span>
            <span className="text-muted font-normal">apply log</span>
          </SheetTitle>
          <SheetDescription asChild>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
              {record && <StatusBadge apply={record} />}
              {record && (
                <span>
                  commit <CommitHash rev={record.commit} />
                </span>
              )}
              {record?.started_at != null && (
                <span>
                  started {ago(record.started_at, now)} ·{" "}
                  {durationLabel(record.started_at, record.finished_at, now)}
                </span>
              )}
              <span className="font-mono">{(view?.applyId ?? "").slice(0, 8)}</span>
            </div>
          </SheetDescription>
          {record?.message && record.state !== "ok" && (
            <p className="text-fg mt-1 font-mono text-xs break-words">{record.message}</p>
          )}
        </SheetHeader>
        <pre
          ref={scroller}
          onScroll={(e) => {
            const el = e.currentTarget;
            pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 24;
          }}
          className="bg-log-bg text-log-fg m-0 min-h-0 flex-1 overflow-auto px-4 py-3.5 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap"
        >
          {error && log === null ? (
            <span className="text-log-cmd">Could not load the log: {error}</span>
          ) : log === null ? (
            <span className="opacity-75">Loading log</span>
          ) : log.text === "" ? (
            <span className="opacity-75">The log is empty so far.</span>
          ) : (
            log.text
          )}
        </pre>
        <div className="border-line text-muted flex items-center gap-2 border-t px-4 py-2 text-xs">
          {running ? (
            <>
              <Building aria-hidden="true" className="text-info size-3.5 animate-spin" />
              Following live
            </>
          ) : (
            <>Finished</>
          )}
          {log && <span className="ml-auto font-mono">{(log.bytes / 1024).toFixed(1)} KiB</span>}
        </div>
      </SheetContent>
    </Sheet>
  );
}
