import { Logo } from "@/components/brand/Logo";
import { ThemeToggle } from "@/components/brand/ThemeToggle";
import { LivePill } from "@/components/fleet/LivePill";
import { NavTabs } from "@/components/layout/NavTabs";
import type { FeedStatus } from "@/connection/feed";
import { Machines, Viewer } from "@/icons";
import { count } from "@/lib/format";
import type { Route } from "@/lib/route";

/**
 * Wordmark and fleet size on the left; feed state, viewer and theme toggle on the right; the
 * page tabs under that row.
 */
export function PageHeader({
  feed,
  viewer,
  hostCount,
  route,
  downCount,
}: {
  feed: FeedStatus;
  /** Who the server says is looking (null before the first snapshot). */
  viewer: string | null;
  hostCount: number | null;
  route: Route;
  /** Services that are down (the count on the Services tab). */
  downCount: number;
}) {
  return (
    <header className="flex min-w-0 flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <Logo>
          {hostCount !== null && (
            <>
              <Machines aria-hidden="true" className="size-4" />
              <span>{count(hostCount, "machine")}</span>
            </>
          )}
        </Logo>
        <div className="flex flex-wrap items-center gap-2.5">
          <LivePill feed={feed} />
          {viewer && (
            <span
              title={`Signed in as ${viewer}`}
              className="border-line bg-panel inline-flex items-center gap-1.5 rounded-full border px-2.5 py-[5px] text-[12.5px] whitespace-nowrap"
            >
              <Viewer aria-hidden="true" className="size-4" />
              <span className="sr-only">Signed in as </span>
              {viewer}
            </span>
          )}
          <ThemeToggle />
        </div>
      </div>
      <NavTabs route={route} downCount={downCount} />
    </header>
  );
}

/** Shown above everything when the server runs its demo. */
export function DemoBanner() {
  return (
    <div className="border-accent-line bg-accent-soft text-accent border-b px-4 py-1.5 text-center text-xs font-medium tracking-wide">
      Demo · simulated machines. Nothing here touches a real machine.
    </div>
  );
}
