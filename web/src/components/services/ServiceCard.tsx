import type { ServiceView } from "@/bindings/ServiceView";
import { OnlineState } from "@/components/fleet/OnlineState";
import { ServiceIcon } from "@/components/services/ServiceIcon";
import { Badge } from "@/components/ui/badge";
import { External } from "@/icons";
import { cn } from "@/lib/cn";
import { serviceLabel, serviceSubLine } from "@/lib/format";
import { serviceStyle, toneStripe } from "@/lib/tones";

/**
 * One service: its mark, the title linking out, the URL, what it is, its check state and the
 * machine it runs on. The left-edge stripe is the service's tone, as on machine rows.
 */
export function ServiceCard({ service: s, now }: { service: ServiceView; now: number }) {
  const { tone, icon: Icon, spin } = serviceStyle(s);
  const sub = serviceSubLine(s, now);
  return (
    <li
      data-tone={tone}
      data-service={s.id}
      className={cn("border-line bg-panel flex min-w-0 flex-col gap-2.5 rounded-md border p-4", toneStripe[tone])}
    >
      <div className="flex min-w-0 items-start gap-3">
        <ServiceIcon icon={s.icon} title={s.title} />
        <div className="min-w-0 flex-1">
          <a
            href={s.url}
            target="_blank"
            rel="noopener noreferrer"
            className="inline-flex max-w-full items-center gap-1.5 text-sm font-semibold hover:underline"
          >
            <span className="truncate">{s.title}</span>
            <External aria-hidden="true" className="text-muted size-3.5 shrink-0" />
          </a>
          <span title={s.url} className="text-muted block truncate font-mono text-[12px]">
            {s.url}
          </span>
        </div>
      </div>
      {s.description && <p className="text-muted m-0 min-w-0 text-[12px]">{s.description}</p>}
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-1.5">
        <Badge tone={tone} data-state={s.check.state} className="whitespace-normal">
          <Icon aria-hidden="true" className={spin ? "animate-spin" : undefined} />
          <span className="min-w-0">{serviceLabel(s)}</span>
        </Badge>
        <span className="text-muted inline-flex min-w-0 flex-wrap items-center gap-x-2 text-[12.5px]">
          <span className="min-w-0 break-all">
            on <span className="text-fg font-medium">{s.host}</span>
          </span>
          {s.host_online !== null && <OnlineState host={{ online: s.host_online, last_seen: null }} now={now} />}
        </span>
      </div>
      {sub && <p className="text-muted m-0 text-[12px]">{sub}</p>}
    </li>
  );
}
