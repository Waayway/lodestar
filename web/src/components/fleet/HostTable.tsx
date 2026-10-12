import type { HostView } from "@/bindings/HostView";
import type { PromoteChannels } from "@/bindings/PromoteChannels";
import { HostRow } from "@/components/fleet/HostRow";
import { EmptyState } from "@/components/layout/EmptyState";
import { Table, TableBody, TableHead, TableHeader, TableRow } from "@/components/ui/table";
import { channelRole } from "@/lib/format";

/** Every machine, by channel then name. Scrolls sideways inside its box on narrow screens. */
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
    <Table className="min-w-[1120px]">
      <TableHeader>
        <TableRow>
          <TableHead>Machine</TableHead>
          <TableHead>Channel</TableHead>
          <TableHead>Status</TableHead>
          <TableHead>Running</TableHead>
          <TableHead>Behind</TableHead>
          <TableHead>Last activation</TableHead>
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
  );
}
