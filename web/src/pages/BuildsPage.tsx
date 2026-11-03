import type { BuildView } from "@/bindings/BuildView";
import type { BuildsView } from "@/bindings/BuildsView";
import type { HostBuildView } from "@/bindings/HostBuildView";
import { ActGate } from "@/components/fleet/ActGate";
import { CommitHash } from "@/components/fleet/CommitHash";
import { EmptyState } from "@/components/layout/EmptyState";
import { ErrorState } from "@/components/layout/ErrorState";
import { PageShell } from "@/components/layout/PageShell";
import { Section } from "@/components/layout/Section";
import { Button } from "@/components/ui/button";
import { Card, CardHeader } from "@/components/ui/card";
import { useActions, useRecentBuilds } from "@/connection/hooks";
import { Builds, Disconnected } from "@/icons";
import { count } from "@/lib/format";
import { ago, useNow } from "@/lib/time";
import { buildStyle, toneText } from "@/lib/tones";

/**
 * The Builds tab: what the builder made of the newest commits, every Linux host with its store
 * path or the log of its failed build, and Retry build for a commit with failures.
 */
export function BuildsPage() {
  const { view, error } = useRecentBuilds();
  const now = useNow(15_000);
  return (
    <PageShell route="builds" loading="Loading builds">
      {() =>
        view === null ? (
          error === null ? (
            <EmptyState title="Loading builds" />
          ) : (
            <ErrorState icon={Disconnected} title="Can't load the builds. Retrying.">
              {error}
            </ErrorState>
          )
        ) : (
          <BuildsList view={view} now={now} />
        )
      }
    </PageShell>
  );
}

/** The builds, newest first, or why there are none. */
export function BuildsList({ view, now }: { view: BuildsView; now: number }) {
  if (!view.enabled && view.builds.length === 0) {
    return (
      <EmptyState title="The build gate is off">
        Machines build their own systems. Turn it on with{" "}
        <code className="font-mono">services.lodestar-server.builds.enable</code>.
      </EmptyState>
    );
  }
  if (view.builds.length === 0) {
    return <EmptyState title="No builds yet">A build starts when a channel gets a new commit.</EmptyState>;
  }
  return (
    <Section icon={Builds} title="Builds">
      <ul className="m-0 flex list-none flex-col gap-3 p-0">
        {view.builds.map((b) => (
          <li key={b.commit}>
            <BuildCard build={b} now={now} />
          </li>
        ))}
      </ul>
    </Section>
  );
}

type BuildKind = "building" | "ok" | "failed";

function buildKind(b: BuildView): BuildKind {
  if (b.building.length > 0) return "building";
  return b.hosts.some((h) => !h.ok) ? "failed" : "ok";
}

/** One commit: its state, the channels it belongs to, and every host's result. */
function BuildCard({ build: b, now }: { build: BuildView; now: number }) {
  const { retryBuild } = useActions();
  const kind = buildKind(b);
  const failed = b.hosts.filter((h) => !h.ok);
  const { tone, icon: Icon, spin } = buildStyle[kind];
  const text =
    kind === "building"
      ? `Building for ${b.building.join(", ")}…`
      : kind === "failed"
        ? `${failed.length} of ${count(b.hosts.length, "machine")} failed`
        : `Built ${count(b.hosts.length, "machine")}`;
  return (
    <Card data-build-commit={b.commit} data-build={kind}>
      <CardHeader>
        <div className="flex min-w-0 items-baseline gap-2">
          <CommitHash rev={b.commit} />
          <span className="text-muted min-w-0 truncate" title={b.subject ?? undefined}>
            {b.subject ?? "Commit not in the mirror"}
          </span>
        </div>
        {b.finished_at !== null && kind !== "building" && (
          <span className="text-muted shrink-0 text-[12.5px]">{ago(b.finished_at, now)}</span>
        )}
      </CardHeader>
      <div className="flex flex-wrap items-center gap-2 text-[12.5px]">
        <span className={`inline-flex items-center gap-1.5 font-medium ${toneText[tone]}`}>
          <Icon aria-hidden="true" className={spin ? "size-3.5 animate-spin" : "size-3.5"} />
          {text}
        </span>
        {b.head_of.length > 0 && <span className="text-muted">· head of {b.head_of.join(", ")}</span>}
        {b.cached_for.length > 0 && <span className="text-muted">· in the cache for {b.cached_for.join(", ")}</span>}
        {kind === "failed" && (
          <ActGate>
            {(gate) => (
              <Button
                variant="outline"
                size="sm"
                disabled={gate.disabled}
                title={gate.title}
                onClick={gate.disabled ? undefined : () => retryBuild(b.commit)}
              >
                Retry build
              </Button>
            )}
          </ActGate>
        )}
      </div>
      {b.hosts.length > 0 && (
        <ul className="border-line m-0 flex list-none flex-col border-t p-0">
          {b.hosts.map((h) => (
            <HostBuildRow key={h.name} host={h} />
          ))}
        </ul>
      )}
    </Card>
  );
}

/** A host's result: its store path, or the log of its failed build behind "Show log". */
function HostBuildRow({ host: h }: { host: HostBuildView }) {
  const { tone, icon: Icon } = buildStyle[h.ok ? "ok" : "failed"];
  return (
    <li data-host={h.name} className="border-line flex min-w-0 flex-col gap-1.5 border-b py-2 last:border-b-0">
      <div className="flex min-w-0 items-center gap-2 text-[12.5px]">
        <span className={`inline-flex shrink-0 items-center gap-1.5 font-medium ${toneText[tone]}`}>
          <Icon aria-hidden="true" className="size-3.5" />
          <span className="sr-only">{h.ok ? "Built" : "Failed"}: </span>
          <span className="text-fg">{h.name}</span>
        </span>
        {h.path !== null && (
          <span className="text-muted min-w-0 truncate font-mono text-xs" title={h.path}>
            {h.path}
          </span>
        )}
      </div>
      {!h.ok && h.log !== null && (
        <details className="text-[12.5px]">
          <summary className="text-muted hover:text-fg cursor-pointer">Show log</summary>
          <pre className="bg-log-bg text-log-fg mt-1.5 mb-0 max-h-96 overflow-auto rounded-sm px-3 py-2.5 font-mono text-xs leading-relaxed break-all whitespace-pre-wrap">
            {h.log}
          </pre>
        </details>
      )}
    </li>
  );
}
