import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import type { ActivationEvent } from "@/bindings/ActivationEvent";
import type { CheckoutStatus } from "@/bindings/CheckoutStatus";
import type { HostView } from "@/bindings/HostView";
import { BehindCount } from "@/components/fleet/BehindCount";
import { ChannelPill } from "@/components/fleet/ChannelPill";
import { CommitHash } from "@/components/fleet/CommitHash";
import { OnlineState } from "@/components/fleet/OnlineState";
import { OsIcon } from "@/components/fleet/OsIcon";
import { StatusBadge } from "@/components/fleet/StatusBadge";
import { Button } from "@/components/ui/button";
import { TableCell, TableRow } from "@/components/ui/table";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useActions } from "@/connection/hooks";
import { Apply, Log, Queued } from "@/icons";
import { cn } from "@/lib/cn";
import {
  type ChannelRole,
  checkoutFlags,
  hostTone,
  osLabel,
  shortRev,
  stripDirty,
  triggerLabel,
} from "@/lib/format";
import { ago, durationLabel } from "@/lib/time";

/** One machine in the table: what it runs, how its checkout looks, and its last apply. */
export function HostRow({
  host: h,
  now,
  channelRole,
}: {
  host: HostView;
  now: number;
  /** How the host's channel pill is drawn (see `channelRole`). */
  channelRole: ChannelRole;
}) {
  const { applyHost, showLog } = useActions();
  const [sending, setSending] = useState(false);
  const applying = h.last_apply?.state === "running";
  // The server re-sends a desired apply the agent never got once the host answers again.
  // While the host is on, a fresh press is simply in flight; that isn't worth a line.
  const queued =
    h.online !== "on" &&
    h.desired_apply_id !== null &&
    h.last_apply?.apply_id !== h.desired_apply_id;
  const flakePath = h.checkout?.flake_path ?? null;

  async function apply() {
    setSending(true);
    try {
      await applyHost(h.name);
    } finally {
      setSending(false);
    }
  }

  return (
    <TableRow data-tone={hostTone(h)} data-host={h.name}>
      <TableCell>
        <div className="flex items-center gap-3">
          <OsIcon os={h.os} />
          <div>
            <div className="font-semibold">{h.name}</div>
            <Sub>{osLabel(h.os)}</Sub>
          </div>
        </div>
      </TableCell>

      <TableCell>
        <ChannelPill name={h.channel} role={channelRole} />
      </TableCell>

      <TableCell>
        <OnlineState host={h} now={now} />
      </TableCell>

      <TableCell>
        {h.running === null ? (
          <span className="text-muted">unknown</span>
        ) : (
          <div className="flex items-center gap-1.5">
            <CommitHash rev={stripDirty(h.running)} />
            {h.running_dirty && (
              <span className="bg-warn-soft text-warn rounded-full px-1.5 py-px text-[11px] font-semibold">
                dirty
              </span>
            )}
          </div>
        )}
        <Checkout checkout={h.checkout} running={h.running} />
      </TableCell>

      <TableCell>
        <BehindCount behind={h.behind} />
      </TableCell>

      <TableCell className="whitespace-nowrap">
        <Activation event={h.last_activation} now={now} />
      </TableCell>

      <TableCell className="max-w-[320px]">
        {h.last_apply?.message && h.last_apply.state !== "ok" ? (
          <Tooltip>
            <TooltipTrigger asChild>
              <span tabIndex={0} className="inline-flex max-w-full min-w-0">
                <StatusBadge apply={h.last_apply} flakePath={flakePath} />
              </span>
            </TooltipTrigger>
            <TooltipContent className="max-w-sm font-mono">{h.last_apply.message}</TooltipContent>
          </Tooltip>
        ) : (
          <StatusBadge apply={h.last_apply} flakePath={flakePath} />
        )}
        {h.last_apply && (
          <Sub className="whitespace-nowrap">
            <span className="font-mono">{shortRev(h.last_apply.commit)}</span>
            {h.last_apply.started_at !== null && (
              <>
                {" "}
                · {ago(h.last_apply.started_at, now)} ·{" "}
                {durationLabel(h.last_apply.started_at, h.last_apply.finished_at, now)}
              </>
            )}
          </Sub>
        )}
        {queued && (
          <Sub className="text-info flex items-center gap-1">
            <Queued aria-hidden="true" className="size-3.5" />
            {h.online === "off"
              ? `Queued: sent when ${h.name} comes online`
              : "Queued: sent when the agent answers"}
          </Sub>
        )}
      </TableCell>

      <TableCell>
        <div className="flex justify-end gap-1.5">
          {h.last_apply && (
            <Button
              variant="outline"
              size="icon-sm"
              aria-label={`Show log for ${h.name}`}
              title="Show log"
              onClick={() => showLog(h.name, h.last_apply!.apply_id)}
            >
              <Log aria-hidden="true" />
            </Button>
          )}
          <Button
            size="sm"
            variant={h.behind === 0 ? "outline" : "default"}
            disabled={sending || applying}
            onClick={apply}
          >
            <Apply aria-hidden="true" />
            {sending ? "Sending" : applying ? "Applying" : "Apply newest"}
          </Button>
        </div>
      </TableCell>
    </TableRow>
  );
}

function Sub({ className, children }: { className?: string; children: ComponentChildren }) {
  return <div className={cn("text-muted mt-[3px] text-xs", className)}>{children}</div>;
}

function Checkout({ checkout: c, running }: { checkout: CheckoutStatus | null; running: string | null }) {
  if (c === null) return <Sub>checkout unknown</Sub>;
  const flags = checkoutFlags(c, running);
  return (
    <Sub className="whitespace-nowrap">
      <span className="font-mono">
        {c.branch ?? "detached"}
        {c.head && <> @ {shortRev(c.head)}</>}
      </span>
      {" · "}
      {flags.length === 0
        ? "clean"
        : flags.map((f, i) => (
            <span key={f.text} className={cn(f.warn && "text-warn font-medium")}>
              {i > 0 && " · "}
              {f.text}
            </span>
          ))}
    </Sub>
  );
}

function Activation({ event: e, now }: { event: ActivationEvent | null; now: number }) {
  if (e === null) return <span className="text-muted">none recorded</span>;
  return (
    <>
      <div className="text-[13px]">{ago(e.time, now)}</div>
      <Sub>
        <span className="font-mono">{shortRev(e.revision)}</span> · {triggerLabel(e)}
        {e.generation !== null && <> · gen {e.generation}</>}
      </Sub>
    </>
  );
}
