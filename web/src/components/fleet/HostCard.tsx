import type { ComponentChildren } from "preact";
import type { HostView } from "@/bindings/HostView";
import { BehindCount } from "@/components/fleet/BehindCount";
import {
  ActivationLine,
  HostActions,
  hostStripe,
  LastApplyInfo,
  MachineIdentity,
  RunningInfo,
} from "@/components/fleet/HostParts";
import { OnlineState } from "@/components/fleet/OnlineState";
import { cn } from "@/lib/cn";
import type { ChannelRole } from "@/lib/format";

/** One machine as a card (below 900 px): the table row's information stacked, actions last. */
export function HostCard({
  host: h,
  now,
  channelRole,
}: {
  host: HostView;
  now: number;
  channelRole: ChannelRole;
}) {
  const stripe = hostStripe(h);
  return (
    <li
      data-tone={stripe.tone}
      data-host={h.name}
      className={cn("border-line bg-panel flex min-w-0 flex-col gap-3 rounded-md border p-4", stripe.className)}
    >
      <div className="flex flex-wrap items-start justify-between gap-x-3 gap-y-2">
        <MachineIdentity host={h} channelRole={channelRole} />
        <OnlineState host={h} now={now} className="whitespace-normal" />
      </div>
      <dl className="m-0 grid grid-cols-[auto_minmax(0,1fr)] gap-x-4 gap-y-2.5">
        <Field label="Running">
          <RunningInfo host={h} />
        </Field>
        <Field label="Behind">
          <BehindCount behind={h.behind} />
        </Field>
        <Field label="Last apply">
          <LastApplyInfo host={h} now={now} />
        </Field>
        <Field label="Last activation">
          <span className="text-[13px]">
            <ActivationLine host={h} now={now} />
          </span>
        </Field>
      </dl>
      <HostActions host={h} wide />
    </li>
  );
}

function Field({ label, children }: { label: string; children: ComponentChildren }) {
  return (
    <>
      <dt className="text-muted font-display tracking-label pt-0.5 text-[11px] font-medium uppercase">{label}</dt>
      <dd className="m-0 min-w-0">{children}</dd>
    </>
  );
}
