import type { ComponentChildren } from "preact";
import type { Snapshot } from "@/bindings/Snapshot";
import { EmptyState } from "@/components/layout/EmptyState";
import { ErrorState } from "@/components/layout/ErrorState";
import { DemoBanner, PageHeader } from "@/components/layout/PageHeader";
import { useFleet, useServices, useViewer } from "@/connection/hooks";
import { Disconnected, Forbidden } from "@/icons";
import type { Route } from "@/lib/route";

/**
 * The frame both pages share: the demo banner, the page width and padding, the header with
 * the tabs, and what to show while there is no snapshot (no access, server unreachable,
 * loading). `children` renders the page once the first snapshot is in.
 */
export function PageShell({
  route,
  loading,
  children,
  after,
}: {
  route: Route;
  /** The quiet line while the first snapshot loads ("Loading machines"). */
  loading: string;
  children: (snap: Snapshot) => ComponentChildren;
  /** Rendered after the page frame (sheets and dialogs). */
  after?: ComponentChildren;
}) {
  const { snapshot: snap, feed } = useFleet();
  const services = useServices();
  const me = useViewer();
  const downCount = services?.filter((s) => s.check.state === "down").length ?? 0;
  return (
    <>
      {snap?.demo && <DemoBanner />}
      <div className="px-[clamp(16px,4vw,48px)] pt-7 pb-12">
        <div className="mx-auto flex max-w-[1240px] min-w-0 flex-col gap-[22px]">
          <PageHeader
            feed={feed}
            viewer={snap?.viewer ?? me?.viewer ?? null}
            session={me?.session ?? false}
            hostCount={snap?.hosts.length ?? null}
            route={route}
            downCount={downCount}
          />
          <main className="flex min-w-0 flex-col gap-[22px]">
            {feed.kind === "forbidden" ? (
              <ErrorState icon={Forbidden} title="You don't have access to this dashboard">
                The Lodestar server only lets in its allowed Tailscale users and tagged devices.
              </ErrorState>
            ) : snap === null ? (
              feed.kind === "waiting" ? (
                <ErrorState icon={Disconnected} title="Can't reach the Lodestar server. Retrying." />
              ) : (
                <EmptyState title={loading} />
              )
            ) : (
              children(snap)
            )}
          </main>
        </div>
      </div>
      {after}
    </>
  );
}
