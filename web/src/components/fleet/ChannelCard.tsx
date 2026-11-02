import { useState } from "preact/hooks";
import type { ChannelBuildView } from "@/bindings/ChannelBuildView";
import type { ChannelView } from "@/bindings/ChannelView";
import type { HostView } from "@/bindings/HostView";
import { ActGate } from "@/components/fleet/ActGate";
import { ChannelPill } from "@/components/fleet/ChannelPill";
import { CommitHash } from "@/components/fleet/CommitHash";
import { Button } from "@/components/ui/button";
import { Card, CardHeader } from "@/components/ui/card";
import { useActions } from "@/connection/hooks";
import { Apply } from "@/icons";
import { type ChannelRole, count } from "@/lib/format";
import { buildStyle, toneText } from "@/lib/tones";

/** A channel's newest commit and the button that applies it to every machine on it. */
export function ChannelCard({
  channel: c,
  hosts,
  role,
}: {
  channel: ChannelView;
  /** The machines on this channel. */
  hosts: HostView[];
  role: ChannelRole;
}) {
  const { applyChannel } = useActions();
  const [sending, setSending] = useState(false);
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
    <Card data-channel={c.name}>
      <CardHeader>
        <ChannelPill name={c.name} role={role} />
        <span className="text-muted text-[12.5px]">
          {count(hosts.length, "machine")}
          {behind > 0 && <span className="text-warn font-medium"> · {behind} behind</span>}
        </span>
      </CardHeader>
      {c.head === null ? (
        <span className="text-muted">No head: branch not in the mirror</span>
      ) : (
        <div className="flex min-w-0 items-baseline gap-2">
          <CommitHash rev={c.head} />
          <span className="text-muted min-w-0 truncate" title={c.head_subject ?? undefined}>
            {c.head_subject}
          </span>
        </div>
      )}
      {c.build !== null && <BuildLine build={c.build} />}
      <ActGate className="inline-flex self-start">
        {(gate) => (
          <Button
            disabled={sending || c.head === null || hosts.length === 0 || gate.disabled}
            title={gate.title}
            onClick={gate.disabled ? undefined : applyAll}
          >
            <Apply aria-hidden="true" />
            {sending ? "Sending" : `Apply newest to all on ${c.name}`}
          </Button>
        )}
      </ActGate>
    </Card>
  );
}

/** The head's build: what it is doing or how it ended, and Retry build when it failed. */
function BuildLine({ build }: { build: ChannelBuildView }) {
  const { retryBuild } = useActions();
  const kind = build.state === "building" || build.state === "failed" ? build.state : "ok";
  const { tone, icon: Icon, spin } = buildStyle[kind];
  const text =
    kind === "building"
      ? "Building on the builder…"
      : kind === "failed"
        ? `Build failed: ${build.failed_hosts.join(", ")}`
        : "Built";
  return (
    <div data-build={kind} className="flex flex-wrap items-center gap-2 text-[12.5px]">
      <span className={`inline-flex items-center gap-1.5 font-medium ${toneText[tone]}`}>
        <Icon aria-hidden="true" className={spin ? "size-3.5 animate-spin" : "size-3.5"} />
        {text}
      </span>
      {kind === "failed" && (
        <ActGate>
          {(gate) => (
            <Button
              variant="outline"
              size="sm"
              disabled={gate.disabled}
              title={gate.title}
              onClick={gate.disabled ? undefined : () => retryBuild(build.commit)}
            >
              Retry build
            </Button>
          )}
        </ActGate>
      )}
    </div>
  );
}
