import { useState } from "preact/hooks";
import type { ChannelView } from "@/bindings/ChannelView";
import type { Snapshot } from "@/bindings/Snapshot";
import { PromoteDialog } from "@/components/PromoteDialog";
import { Button } from "@/components/ui/button";
import { useActions } from "@/connection/hooks";
import { shortRev } from "@/lib/format";

export function ChannelBar({ snapshot }: { snapshot: Snapshot }) {
  return (
    <section className="flex flex-col gap-3 lg:flex-row lg:items-stretch">
      <div className="grid flex-1 gap-3 sm:grid-cols-2">
        {snapshot.channels.map((c) => (
          <ChannelCard key={c.name} channel={c} snapshot={snapshot} />
        ))}
      </div>
      {snapshot.promote_candidate !== null && snapshot.promote !== null && (
        <div className="bg-card flex flex-col justify-between gap-3 rounded-lg border p-4 lg:w-72">
          <div>
            <div className="text-muted-foreground text-[11px] font-medium tracking-wider uppercase">
              Ready to promote
            </div>
            <div className="mt-1 text-sm">
              <span className="font-mono">{snapshot.promote.from}</span> is ahead of{" "}
              <span className="font-mono">{snapshot.promote.to}</span>
            </div>
          </div>
          <PromoteDialog
            commit={snapshot.promote_candidate}
            pair={snapshot.promote}
            channels={snapshot.channels}
          />
        </div>
      )}
    </section>
  );
}

function ChannelCard({
  channel: c,
  snapshot,
}: {
  channel: ChannelView;
  snapshot: Snapshot;
}) {
  const { applyChannel } = useActions();
  const [sending, setSending] = useState(false);
  const hosts = snapshot.hosts.filter((h) => h.channel === c.name);
  const behind = hosts.filter((h) => h.behind !== null && h.behind > 0).length;

  async function applyAll() {
    setSending(true);
    try {
      await applyChannel(c.name);
    } finally {
      setSending(false);
    }
  }

  return (
    <div className="bg-card flex flex-col gap-3 rounded-lg border p-4">
      <div className="flex items-baseline justify-between gap-3">
        <span className="font-mono text-sm font-semibold">{c.name}</span>
        <span className="text-muted-foreground text-xs">
          {hosts.length} {hosts.length === 1 ? "host" : "hosts"}
          {behind > 0 && <span className="text-warn font-medium"> · {behind} behind</span>}
        </span>
      </div>
      <div className="min-w-0 text-sm">
        {c.head === null ? (
          <span className="text-muted-foreground">No head: branch not in the mirror</span>
        ) : (
          <div className="flex min-w-0 items-baseline gap-2">
            <span className="font-mono text-[13px]">{shortRev(c.head)}</span>
            <span className="text-muted-foreground truncate" title={c.head_subject ?? undefined}>
              {c.head_subject}
            </span>
          </div>
        )}
      </div>
      <Button
        variant="outline"
        size="sm"
        className="self-start"
        disabled={sending || c.head === null || hosts.length === 0}
        onClick={applyAll}
      >
        {sending ? "Sending" : (
          <>
            Apply newest to all on <span className="font-mono">{c.name}</span>
          </>
        )}
      </Button>
    </div>
  );
}
