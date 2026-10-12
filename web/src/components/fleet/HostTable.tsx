import type { HostView } from "@/bindings/HostView";
import type { PromoteChannels } from "@/bindings/PromoteChannels";
import { HostCard } from "@/components/fleet/HostCard";
import { HostRow } from "@/components/fleet/HostRow";
import { EmptyState } from "@/components/layout/EmptyState";
import { Table, TableBody, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { channelRole } from "@/lib/format";

/**
 * Every machine, by channel then name: a six-column table from 900 px, a list of cards below.
 * Neither scrolls sideways; CSS shows the one that fits.
 */
export function HostTable({
  hosts,
  now,
  promote,
}: {
  hosts: HostView[];
  now: number;
  promote: PromoteChannels | null;
}) {
  if (hosts.length === 0) return <EmptyState title="No machines yet." />;
  const sorted = [...hosts].sort(
    (a, b) => a.channel.localeCompare(b.channel) || a.name.localeCompare(b.name),
  );
  return (
    <>
      <Table data-layout="table" boxClassName="hidden min-[900px]:block">
        <TableHeader>
          <TableRow>
            <TableHead>Machine</TableHead>
            <TableHead>Status</TableHead>
            <TableHead>Running</TableHead>
            <TableHead>Behind</TableHead>
            <TableHead>Last apply</TableHead>
            <TableHead>
              <span className="sr-only">Actions</span>
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {sorted.map((h) => (
            <HostRow key={h.name} host={h} now={now} channelRole={channelRole(h.channel, promote)} />
          ))}
        </TableBody>
      </Table>
      <ul data-layout="cards" className="m-0 flex list-none flex-col gap-3 p-0 min-[900px]:hidden">
        {sorted.map((h) => (
          <HostCard key={h.name} host={h} now={now} channelRole={channelRole(h.channel, promote)} />
        ))}
      </ul>
    </>
  );
}
