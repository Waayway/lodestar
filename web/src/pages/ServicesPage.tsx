import type { ServiceView } from "@/bindings/ServiceView";
import { StatTile } from "@/components/fleet/StatTile";
import { EmptyState } from "@/components/layout/EmptyState";
import { PageShell } from "@/components/layout/PageShell";
import { Section } from "@/components/layout/Section";
import { ServiceCard } from "@/components/services/ServiceCard";
import { useServices } from "@/connection/hooks";
import { Building, Down, Idle, Services, Up } from "@/icons";
import { count, groupServices, serviceCounts } from "@/lib/format";
import { cn } from "@/lib/cn";
import { useNow } from "@/lib/time";

/**
 * The Services tab: every declared link with its live check, one section per group. Groups
 * share one grid of card columns (1, 2, 3, 4 by width) and are packed densely, so small
 * groups sit side by side.
 */
export function ServicesPage() {
  const services = useServices();
  const now = useNow(15_000);
  return (
    <PageShell route="services" loading="Loading services">
      {() =>
        services === null || services.length === 0 ? (
          <EmptyState title="No services declared">
            Declare them with <code className="font-mono">lodestar.links</code> in a machine's configuration.
          </EmptyState>
        ) : (
          <>
            <Stats services={services} />
            <div className="grid grid-flow-row-dense grid-cols-1 gap-x-3 gap-y-[22px] min-[640px]:grid-cols-2 min-[900px]:grid-cols-3 min-[1200px]:grid-cols-4">
              {groupServices(services).map(([group, list]) => {
                const columns = groupColumns(list.length);
                return (
                  <Section
                    key={group}
                    label={group}
                    icon={Services}
                    className={columns.section}
                    title={
                      <>
                        {group}
                        <span className="text-muted font-sans text-[12px] font-normal tracking-normal normal-case">
                          {count(list.length, "service")}
                        </span>
                      </>
                    }
                  >
                    <ul className={cn("m-0 grid list-none gap-3 p-0", columns.list)}>
                      {list.map((s) => (
                        <ServiceCard key={s.id} service={s} now={now} />
                      ))}
                    </ul>
                  </Section>
                );
              })}
            </div>
          </>
        )
      }
    </PageShell>
  );
}

function Stats({ services }: { services: ServiceView[] }) {
  const c = serviceCounts(services);
  return (
    <section aria-label="Services summary" className="grid grid-cols-2 gap-3 min-[900px]:grid-cols-4">
      <StatTile icon={Up} tone="ok" value={c.up} label="up" />
      <StatTile icon={Down} tone="bad" value={c.down} label="down" />
      <StatTile icon={Building} tone="info" value={c.pending} label="checking" spin={c.pending > 0} />
      {c.unchecked > 0 && <StatTile icon={Idle} tone="neutral" value={c.unchecked} label="not checked" />}
    </section>
  );
}

// Literal class names per breakpoint (Tailwind only generates classes it finds verbatim).
// Index = columns spanned; the page has 1, 2, 3 and 4 columns at these widths.
const SECTION_SPAN = [
  ["col-span-1"],
  ["", "min-[640px]:col-span-1", "min-[640px]:col-span-2"],
  ["", "min-[900px]:col-span-1", "min-[900px]:col-span-2", "min-[900px]:col-span-3"],
  ["", "min-[1200px]:col-span-1", "min-[1200px]:col-span-2", "min-[1200px]:col-span-3", "min-[1200px]:col-span-4"],
];
const LIST_COLUMNS = [
  ["grid-cols-1"],
  ["", "min-[640px]:grid-cols-1", "min-[640px]:grid-cols-2"],
  ["", "min-[900px]:grid-cols-1", "min-[900px]:grid-cols-2", "min-[900px]:grid-cols-3"],
  ["", "min-[1200px]:grid-cols-1", "min-[1200px]:grid-cols-2", "min-[1200px]:grid-cols-3", "min-[1200px]:grid-cols-4"],
];

/**
 * A group's classes: the section spans one page column per card (capped at the columns at
 * each width), and its card list has as many equal columns, so cards line up across groups.
 */
export function groupColumns(cards: number): { section: string; list: string } {
  const pick = (table: string[][]) =>
    table.map((row, index) => row[Math.min(Math.max(cards, 1), index + 1)] ?? row[0]).join(" ");
  return { section: pick(SECTION_SPAN), list: pick(LIST_COLUMNS) };
}
