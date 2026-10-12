import { useEffect, useState } from "preact/hooks";
import { ApplyLogSheet } from "@/components/ApplyLogSheet";
import { ChannelBar } from "@/components/ChannelBar";
import { HostTable } from "@/components/HostTable";
import type { FeedStatus } from "@/connection/feed";
import { useFleet } from "@/connection/hooks";
import { cn } from "@/lib/cn";
import { useNow } from "@/lib/time";

/** The dashboard: composes the sections; data comes from `connection/hooks`. */
export function DashboardPage() {
  const { snapshot: snap, feed } = useFleet();
  const now = useNow(15_000);
  return (
    <>
      <div className="min-h-dvh">
        {snap?.demo && (
          <div className="bg-demo text-demo-foreground border-b border-demo-foreground/15 px-4 py-1.5 text-center text-xs font-medium tracking-wide">
            Demo · simulated hosts. Nothing here touches a real machine.
          </div>
        )}
        <header className="mx-auto flex max-w-[1400px] flex-wrap items-center gap-x-4 gap-y-1 px-4 pt-5 pb-3 sm:px-6">
          <h1 className="flex items-baseline gap-2 text-lg font-semibold tracking-tight">
            <span className="font-mono">lodestar</span>
            {snap && (
              <span className="text-muted-foreground text-sm font-normal">
                {snap.hosts.length} {snap.hosts.length === 1 ? "host" : "hosts"}
              </span>
            )}
          </h1>
          <div className="text-muted-foreground ml-auto flex items-center gap-4 text-xs">
            {snap && <span>Signed in as {snap.viewer}</span>}
            <FeedIndicator feed={feed} />
          </div>
        </header>

        <main className="mx-auto flex max-w-[1400px] flex-col gap-4 px-4 pb-10 sm:px-6">
          {feed.kind === "forbidden" ? (
            <div className="py-16 text-center">
              <p className="text-sm font-medium">You don't have access to this dashboard</p>
              <p className="text-muted-foreground mt-1 text-sm">
                The Lodestar server only lets in its allowed Tailscale users and tagged devices.
              </p>
            </div>
          ) : snap === null ? (
            <p className="text-muted-foreground py-16 text-center text-sm">
              {feed.kind === "waiting" ? "Can't reach the Lodestar server. Retrying." : "Loading hosts"}
            </p>
          ) : (
            <>
              <ChannelBar snapshot={snap} />
              <HostTable hosts={snap.hosts} now={now} />
            </>
          )}
        </main>
      </div>
      <ApplyLogSheet now={now} />
    </>
  );
}

function FeedIndicator({ feed }: { feed: FeedStatus }) {
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
    <span className="flex items-center gap-1.5" role="status">
      <span
        className={cn(
          "size-2 rounded-full",
          feed.kind === "live" ? "bg-ok" : feed.kind === "connecting" ? "bg-running animate-pulse" : "bg-failed",
        )}
      />
      {label}
    </span>
  );
}
