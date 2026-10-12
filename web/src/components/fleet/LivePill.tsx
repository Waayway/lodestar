import { useEffect, useState } from "preact/hooks";
import type { FeedStatus } from "@/connection/feed";
import { cn } from "@/lib/cn";
import { feedStyle, toneText } from "@/lib/tones";

/** What screen readers hear: changes only when the feed's state does, never per second. */
const announced: Record<FeedStatus["kind"], string> = {
  live: "Live",
  connecting: "Connecting",
  waiting: "Disconnected, retrying",
  forbidden: "No access",
};

/** The feed's state in the header: Live, Connecting, Disconnected with a countdown, No access. */
export function LivePill({ feed }: { feed: FeedStatus }) {
  const [, tick] = useState(0);
  useEffect(() => {
    if (feed.kind !== "waiting") return;
    const t = setInterval(() => tick((n) => n + 1), 1000);
    return () => clearInterval(t);
  }, [feed]);
  const label =
    feed.kind === "waiting"
      ? `Disconnected · retry in ${Math.max(0, Math.ceil((feed.retryAt - Date.now()) / 1000))}s`
      : announced[feed.kind];
  const { tone, icon: Icon } = feedStyle[feed.kind];
  return (
    <span
      data-state={feed.kind}
      className={cn(
        "border-line bg-panel inline-flex items-center gap-1.5 rounded-full border px-2.5 py-[5px] text-[12.5px] whitespace-nowrap",
        toneText[tone],
      )}
    >
      {Icon ? (
        <Icon aria-hidden="true" className="size-4" />
      ) : (
        <span aria-hidden="true" className="size-2 rounded-full bg-current" />
      )}
      <span aria-hidden="true">{label}</span>
      <span role="status" className="sr-only">
        {announced[feed.kind]}
      </span>
    </span>
  );
}
