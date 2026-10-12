import { useEffect, useState } from "preact/hooks";
import type { FeedStatus } from "@/connection/feed";
import { Disconnected, Forbidden } from "@/icons";
import { cn } from "@/lib/cn";

/** The feed's state in the header: Live, Connecting, Disconnected with a countdown, No access. */
export function LivePill({ feed }: { feed: FeedStatus }) {
  const [, tick] = useState(0);
  useEffect(() => {
    if (feed.kind !== "waiting") return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [feed]);
  const label =
    feed.kind === "live"
      ? "Live"
      : feed.kind === "connecting"
        ? "Connecting"
        : feed.kind === "forbidden"
          ? "No access"
          : `Disconnected · retry in ${Math.max(0, Math.ceil((feed.retryAt - Date.now()) / 1000))}s`;
  return (
    <span
      role="status"
      data-state={feed.kind}
      className={cn(
        "border-line bg-panel inline-flex items-center gap-1.5 rounded-full border px-2.5 py-[5px] text-[12.5px] whitespace-nowrap",
        feed.kind === "live" && "text-ok",
        feed.kind === "connecting" && "text-info",
        feed.kind === "waiting" && "text-bad",
        feed.kind === "forbidden" && "text-bad",
      )}
    >
      {feed.kind === "live" || feed.kind === "connecting" ? (
        <span aria-hidden="true" className="size-2 rounded-full bg-current" />
      ) : feed.kind === "waiting" ? (
        <Disconnected aria-hidden="true" className="size-4" />
      ) : (
        <Forbidden aria-hidden="true" className="size-4" />
      )}
      {label}
    </span>
  );
}
