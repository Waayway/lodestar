import type { ComponentChildren } from "preact";
import { Builds, Machines, type Icon, Services } from "@/icons";
import { cn } from "@/lib/cn";
import type { Route } from "@/lib/route";
import { toneChip } from "@/lib/tones";

/**
 * The pages: Fleet, Builds and Services (only Services when `fleet` is false). Builds carries
 * how many channels failed to build, Services how many services are down.
 */
export function NavTabs({
  route,
  downCount,
  failedBuilds = 0,
  fleet = true,
}: {
  route: Route;
  downCount: number;
  failedBuilds?: number;
  /** Whether the Fleet tab shows (not for anonymous visitors). */
  fleet?: boolean;
}) {
  return (
    <nav aria-label="Pages" className="border-line flex gap-1 border-b">
      {fleet && (
        <Tab href="#/" icon={Machines} current={route === "fleet"}>
          Fleet
        </Tab>
      )}
      {fleet && (
        <Tab href="#/builds" icon={Builds} current={route === "builds"}>
          Builds
          <Count n={failedBuilds} label="failed" />
        </Tab>
      )}
      <Tab href="#/services" icon={Services} current={route === "services"}>
        Services
        <Count n={downCount} label="down" />
      </Tab>
    </nav>
  );
}

function Tab({
  href,
  icon: Glyph,
  current,
  children,
}: {
  href: string;
  icon: Icon;
  current: boolean;
  children: ComponentChildren;
}) {
  return (
    <a
      href={href}
      aria-current={current ? "page" : undefined}
      className={cn(
        "-mb-px inline-flex items-center gap-2 border-b-2 px-3 py-2 text-[13px] font-semibold",
        current ? "border-fg text-fg" : "text-muted hover:text-fg border-transparent",
      )}
    >
      <Glyph aria-hidden="true" className="size-4" />
      {children}
    </a>
  );
}

/** A red count on a tab ("2 down"); nothing when it is 0. */
function Count({ n, label }: { n: number; label: string }) {
  if (n === 0) return null;
  return (
    <span className={cn("rounded-sm px-1.5 text-[11.5px] leading-[18px] font-semibold tabular-nums", toneChip("bad"))}>
      {n}
      <span className="sr-only"> {label}</span>
    </span>
  );
}
