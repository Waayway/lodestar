import { Logo } from "@/components/brand/Logo";
import { ThemeToggle } from "@/components/brand/ThemeToggle";
import { LivePill } from "@/components/fleet/LivePill";
import { EmptyState } from "@/components/layout/EmptyState";
import { ErrorState } from "@/components/layout/ErrorState";
import type { FeedStatus } from "@/connection/feed";
import { useMeRetryAt, useViewMode } from "@/connection/hooks";
import { Disconnected, Forbidden } from "@/icons";

/**
 * What shows before the server has said who the viewer is (`/api/me`), or when it refuses the
 * viewer: the logo, the state pill and the theme toggle, no tabs (an anonymous visitor never
 * sees a Fleet tab), and one line: loading, can't reach the server (retrying), or no access.
 */
export function StatusPage() {
  const mode = useViewMode();
  const retryAt = useMeRetryAt();
  const status: FeedStatus =
    mode === "forbidden"
      ? { kind: "forbidden" }
      : retryAt !== null
        ? { kind: "waiting", retryAt }
        : { kind: "connecting" };
  return (
    <div className="px-[clamp(16px,4vw,48px)] pt-7 pb-12">
      <div className="mx-auto flex max-w-[1240px] min-w-0 flex-col gap-[22px]">
        <header className="flex flex-wrap items-center justify-between gap-4">
          <Logo />
          <div className="flex flex-wrap items-center gap-2.5">
            <LivePill feed={status} />
            <ThemeToggle />
          </div>
        </header>
        <main className="flex min-w-0 flex-col gap-[22px]">
          {status.kind === "forbidden" ? (
            <ErrorState icon={Forbidden} title="You don't have access to this dashboard">
              The Lodestar server only lets in its allowed Tailscale users and tagged devices.
            </ErrorState>
          ) : status.kind === "waiting" ? (
            <ErrorState icon={Disconnected} title="Can't reach the Lodestar server. Retrying." />
          ) : (
            <EmptyState title="Loading" />
          )}
        </main>
      </div>
    </div>
  );
}
