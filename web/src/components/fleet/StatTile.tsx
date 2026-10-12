import type { Icon } from "@/icons";
import { cn } from "@/lib/cn";
import { type Tone, toneChip } from "@/lib/tones";

/** One number in the fleet summary, with its icon on a soft square. */
export function StatTile({
  icon: Glyph,
  value,
  label,
  tone,
  spin = false,
}: {
  icon: Icon;
  value: number;
  label: string;
  tone: Tone;
  /** Spin the icon (the "updating" tile while something builds). */
  spin?: boolean;
}) {
  return (
    <div className="border-line bg-panel flex items-center gap-3 rounded-md border px-4 py-3.5">
      <div className={cn("grid size-9 shrink-0 place-items-center rounded-sm", toneChip(tone))}>
        <Glyph aria-hidden="true" className={cn("size-[22px]", spin && "animate-spin")} />
      </div>
      <div>
        <b className="font-display block text-2xl leading-tight font-semibold tabular-nums">{value}</b>
        <span className="text-muted text-[12.5px]">{label}</span>
      </div>
    </div>
  );
}
