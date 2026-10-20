import { Logo } from "@/components/brand/Logo";
import { ThemeToggle } from "@/components/brand/ThemeToggle";
import { StatTile } from "@/components/fleet/StatTile";
import { EmptyState } from "@/components/layout/EmptyState";
import { NavTabs } from "@/components/layout/NavTabs";
import { Section } from "@/components/layout/Section";
import { PublicServiceCard } from "@/components/services/PublicServiceCard";
import { Button } from "@/components/ui/button";
import { useActions, usePublicServices } from "@/connection/hooks";
import { Down, Idle, LogIn, Services, Up } from "@/icons";
import { cn } from "@/lib/cn";
import { count, groupServices, publicServiceCounts } from "@/lib/format";
import { useNow } from "@/lib/time";
import { groupColumns } from "@/pages/ServicesPage";

/**
 * What an anonymous visitor gets whatever the route: the logo, the theme toggle and a Log in
 * button, the one Services tab, and the services by group (name, description, state, how
 * long; never a link, URL or host). Groups use the same card grid as the Services tab.
 */
export function PublicServicesPage() {
  const { login } = useActions();
  const services = usePublicServices();
  const now = useNow(15_000);
  return (
    <div className="px-[clamp(16px,4vw,48px)] pt-7 pb-12">
      <div className="mx-auto flex max-w-[1240px] min-w-0 flex-col gap-[22px]">
        <header className="flex min-w-0 flex-col gap-4">
          <div className="flex flex-wrap items-center justify-between gap-4">
            <Logo />
            <div className="flex flex-wrap items-center gap-2.5">
              <ThemeToggle />
              <Button size="sm" onClick={login}>
                <LogIn aria-hidden="true" />
                Log in
              </Button>
            </div>
          </div>
          <NavTabs route="services" downCount={0} fleet={false} />
        </header>
        <main className="flex min-w-0 flex-col gap-[22px]">
          {services === null ? (
            <EmptyState title="Loading services" />
          ) : services.length === 0 ? (
            <EmptyState title="No services declared" />
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
                          <PublicServiceCard key={s.title} service={s} now={now} />
                        ))}
                      </ul>
                    </Section>
                  );
                })}
              </div>
            </>
          )}
        </main>
      </div>
    </div>
  );
}

function Stats({ services }: { services: Parameters<typeof publicServiceCounts>[0] }) {
  const c = publicServiceCounts(services);
  return (
    <section aria-label="Services summary" className="grid grid-cols-3 gap-3">
      <StatTile icon={Up} tone="ok" value={c.up} label="up" />
      <StatTile icon={Down} tone="bad" value={c.down} label="down" />
      <StatTile icon={Idle} tone="neutral" value={c.unknown} label="unknown" />
    </section>
  );
}
