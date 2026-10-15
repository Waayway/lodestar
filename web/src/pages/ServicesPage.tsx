import type { ServiceView } from "@/bindings/ServiceView";
import { StatTile } from "@/components/fleet/StatTile";
import { EmptyState } from "@/components/layout/EmptyState";
import { PageShell } from "@/components/layout/PageShell";
import { Section } from "@/components/layout/Section";
import { ServiceCard } from "@/components/services/ServiceCard";
import { useServices } from "@/connection/hooks";
import { Building, Down, Idle, Services, Up } from "@/icons";
import { count, groupServices, serviceCounts } from "@/lib/format";
import { useNow } from "@/lib/time";

/** The Services tab: every declared link with its live check, one section per group. */
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
            {groupServices(services).map(([group, list]) => (
              <Section
                key={group}
                label={group}
                icon={Services}
                title={
                  <>
                    {group}
                    <span className="text-muted font-sans text-[12px] font-normal tracking-normal normal-case">
                      {count(list.length, "service")}
                    </span>
                  </>
                }
              >
                <ul className="m-0 grid list-none grid-cols-[repeat(auto-fill,minmax(min(280px,100%),1fr))] gap-3 p-0">
                  {list.map((s) => (
                    <ServiceCard key={s.id} service={s} now={now} />
                  ))}
                </ul>
              </Section>
            ))}
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
