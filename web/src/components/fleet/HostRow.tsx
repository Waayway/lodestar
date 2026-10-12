import type { HostView } from "@/bindings/HostView";
import { BehindCount } from "@/components/fleet/BehindCount";
import { HostActions, hostStripe, LastApplyInfo, MachineIdentity, RunningInfo } from "@/components/fleet/HostParts";
import { OnlineState } from "@/components/fleet/OnlineState";
import { TableCell, TableRow } from "@/components/ui/table";
import type { ChannelRole } from "@/lib/format";

/** One machine in the table (900 px and up): six columns, sub-lines wrap. */
export function HostRow({
  host: h,
  now,
  channelRole,
}: {
  host: HostView;
  now: number;
  /** How the machine's channel pill is drawn (see `channelRole`). */
  channelRole: ChannelRole;
}) {
  const stripe = hostStripe(h);
  return (
    <TableRow data-tone={stripe.tone} data-host={h.name}>
      <TableCell className={stripe.className}>
        <MachineIdentity host={h} channelRole={channelRole} />
      </TableCell>
      <TableCell>
        <OnlineState host={h} now={now} className="whitespace-normal" />
      </TableCell>
      <TableCell>
        <RunningInfo host={h} />
      </TableCell>
      <TableCell className="whitespace-nowrap">
        <BehindCount behind={h.behind} />
      </TableCell>
      <TableCell className="max-w-[260px]">
        <LastApplyInfo host={h} now={now} />
      </TableCell>
      <TableCell>
        <HostActions host={h} />
      </TableCell>
    </TableRow>
  );
}
