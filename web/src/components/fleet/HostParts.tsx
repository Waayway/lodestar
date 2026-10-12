// The pieces of one machine, shared by the table row (HostRow, 900 px and up) and the card
// (HostCard, below): the same state, words and actions in two layouts.
import type { ComponentChildren } from "preact";
import { useState } from "preact/hooks";
import type { HostView } from "@/bindings/HostView";
import { ChannelPill } from "@/components/fleet/ChannelPill";
import { CommitHash } from "@/components/fleet/CommitHash";
import { OsIcon } from "@/components/fleet/OsIcon";
import { StatusBadge } from "@/components/fleet/StatusBadge";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { useActions } from "@/connection/hooks";
import { Apply, Log, Queued } from "@/icons";
import { cn } from "@/lib/cn";
import {
  activationText,
  applyBadge,
  type ChannelRole,
  checkoutFlags,
  hostTone,
  osLabel,
  shortRev,
  stripDirty,
} from "@/lib/format";
import { ago, durationLabel } from "@/lib/time";
import { hostStyle, toneStripe } from "@/lib/tones";

/** The state stripe class for a machine (none when idle) and its `hostTone`. */
export function hostStripe(h: HostView): { tone: ReturnType<typeof hostTone>; className: string | undefined } {
  const tone = hostTone(h);
  const t = hostStyle[tone];
  return { tone, className: t ? toneStripe[t] : undefined };
}

export function Sub({ className, children }: { className?: string; children: ComponentChildren }) {
  return <div className={cn("text-muted mt-[3px] text-xs", className)}>{children}</div>;
}

/** OS tile, the name (never broken), and under it the OS and the channel pill. */
export function MachineIdentity({ host: h, channelRole }: { host: HostView; channelRole: ChannelRole }) {
  return (
    <div className="flex items-center gap-3">
      <OsIcon os={h.os} />
      <div className="min-w-0">
        <div className="font-semibold whitespace-nowrap">{h.name}</div>
        <div className="text-muted mt-[3px] flex flex-wrap items-center gap-x-1.5 gap-y-1 text-xs">
          <span>{osLabel(h.os)}</span>
          <ChannelPill name={h.channel} role={channelRole} className="px-1.5 py-px text-[11px]" />
        </div>
      </div>
    </div>
  );
}

/** The running commit (and "dirty"), with the checkout line under it (may wrap). */
export function RunningInfo({ host: h }: { host: HostView }) {
  const c = h.checkout;
  const flags = c ? checkoutFlags(c, h.running) : [];
  return (
    <>
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
      {c === null ? (
        <Sub>checkout unknown</Sub>
      ) : (
        <Sub>
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
      )}
    </>
  );
}

/** The last activation as a line: "2h ago · f63ae42 · Lodestar apply · gen 11". */
export function ActivationLine({ host: h, now }: { host: HostView; now: number }) {
  return (
    <span>
      {h.last_activation === null ? "none recorded" : activationText(h.last_activation, now)}
    </span>
  );
}

/**
 * The last apply: its badge (whose tooltip adds the full label, the agent's message and the
 * last activation), when it started and how long it ran, and a queued note.
 */
export function LastApplyInfo({ host: h, now }: { host: HostView; now: number }) {
  const flakePath = h.checkout?.flake_path ?? null;
  const a = h.last_apply;
  // The server re-sends a desired apply the agent never got once the host answers again.
  // While the host is on, a fresh press is simply in flight; that isn't worth a line.
  const queued = h.online !== "on" && h.desired_apply_id !== null && a?.apply_id !== h.desired_apply_id;
  return (
    <>
      <Tooltip>
        <TooltipTrigger asChild>
          <span tabIndex={0} className="inline-flex max-w-full min-w-0">
            <StatusBadge apply={a} flakePath={flakePath} wrap />
          </span>
        </TooltipTrigger>
        <TooltipContent className="flex max-w-sm flex-col gap-1">
          <span>{applyBadge(a, flakePath).text}</span>
          {a?.message && a.state !== "ok" && <span className="font-mono">{a.message}</span>}
          <span>
            Last activation: <ActivationLine host={h} now={now} />
          </span>
        </TooltipContent>
      </Tooltip>
      {a && (
        <Sub>
          <span className="font-mono">{shortRev(a.commit)}</span>
          {a.started_at !== null && (
            <>
              {" "}
              · {ago(a.started_at, now)} · {durationLabel(a.started_at, a.finished_at, now)}
            </>
          )}
        </Sub>
      )}
      {queued && (
        <Sub className="text-info flex items-center gap-1">
          <Queued aria-hidden="true" className="size-3.5 shrink-0" />
          {h.online === "off" ? `Queued: sent when ${h.name} comes online` : "Queued: sent when the agent answers"}
        </Sub>
      )}
    </>
  );
}

/** Show the last apply's log, and apply newest. `wide` stretches the apply button (cards). */
export function HostActions({ host: h, wide = false }: { host: HostView; wide?: boolean }) {
  const { applyHost, showLog } = useActions();
  const [sending, setSending] = useState(false);
  const applying = h.last_apply?.state === "running";
  async function apply() {
    setSending(true);
    try {
      await applyHost(h.name);
    } finally {
      setSending(false);
    }
  }
  return (
    <div className={cn("flex gap-1.5", wide ? "w-full" : "justify-end")}>
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
        className={wide ? "flex-1" : undefined}
      >
        <Apply aria-hidden="true" />
        {sending ? "Sending" : applying ? "Applying" : "Apply newest"}
      </Button>
    </div>
  );
}
