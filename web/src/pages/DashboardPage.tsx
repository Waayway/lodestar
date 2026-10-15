import { ApplyLogSheet } from "@/components/fleet/ApplyLogSheet";
import { AuditList } from "@/components/fleet/AuditList";
import { ChannelCard } from "@/components/fleet/ChannelCard";
import { HostTable } from "@/components/fleet/HostTable";
import { PromoteCard } from "@/components/fleet/PromoteCard";
import { StatTile } from "@/components/fleet/StatTile";
import { PageShell } from "@/components/layout/PageShell";
import { Section } from "@/components/layout/Section";
import { useAudit } from "@/connection/hooks";
import { Applied, Attention, Audit, Building, Channel, Machines } from "@/icons";
import { cn } from "@/lib/cn";
import { channelRole, fleetCounts } from "@/lib/format";
import { useNow } from "@/lib/time";
import type { Snapshot } from "@/bindings/Snapshot";

/** The dashboard: composes the sections; data comes from `connection/hooks`. */
export function DashboardPage() {
  const audit = useAudit();
  const now = useNow(15_000);
  return (
    <PageShell route="fleet" loading="Loading machines" after={<ApplyLogSheet now={now} />}>
      {(snap) => (
        <>
          <Stats snap={snap} />
          <Channels snap={snap} />
          <Section title="Machines" icon={Machines}>
            <HostTable hosts={snap.hosts} now={now} promote={snap.promote} />
          </Section>
          <Section title="Audit" icon={Audit}>
            <AuditList events={audit} now={now} />
          </Section>
        </>
      )}
    </PageShell>
  );
}

function Stats({ snap }: { snap: Snapshot }) {
  const c = fleetCounts(snap.hosts);
  return (
    <section aria-label="Fleet summary" className="grid grid-cols-2 gap-3 min-[900px]:grid-cols-4">
      <StatTile icon={Machines} tone="accent" value={c.total} label={c.total === 1 ? "machine" : "machines"} />
      <StatTile icon={Applied} tone="ok" value={c.upToDate} label="up to date" />
      <StatTile icon={Building} tone="info" value={c.updating} label="updating" spin={c.updating > 0} />
      <StatTile icon={Attention} tone="bad" value={c.attention} label="need attention" />
    </section>
  );
}

/** Channel cards, with the promote card between the promote pair when there is a candidate. */
function Channels({ snap }: { snap: Snapshot }) {
  const pair = snap.promote;
  const showPromote = snap.promote_candidate !== null && pair !== null;
  const from = pair && snap.channels.find((c) => c.name === pair.from);
  const to = pair && snap.channels.find((c) => c.name === pair.to);
  const between = showPromote && from && to;
  const rest = between ? snap.channels.filter((c) => c !== from && c !== to) : snap.channels;
  const card = (c: (typeof snap.channels)[number]) => (
    <ChannelCard
      key={c.name}
      channel={c}
      hosts={snap.hosts.filter((h) => h.channel === c.name)}
      role={channelRole(c.name, pair)}
    />
  );
  const promoteCard = showPromote && (
    <PromoteCard candidate={snap.promote_candidate!} pair={pair!} channels={snap.channels} />
  );
  return (
    <Section title="Channels" icon={Channel}>
      <div
        className={cn(
          "grid grid-cols-1 items-stretch gap-3",
          between && rest.length === 0
            ? "min-[900px]:grid-cols-[minmax(0,1fr)_auto_minmax(0,1fr)]"
            : "min-[900px]:grid-cols-[repeat(auto-fit,minmax(280px,1fr))]",
        )}
      >
        {between ? (
          <>
            {card(from)}
            {promoteCard}
            {card(to)}
            {rest.map(card)}
          </>
        ) : (
          <>
            {snap.channels.map(card)}
            {promoteCard}
          </>
        )}
      </div>
    </Section>
  );
}
