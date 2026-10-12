import type { ComponentChildren } from "preact";
import type { Icon } from "@/icons";
import { cn } from "@/lib/cn";
import { type Tone, toneChip } from "@/lib/tones";

/** The page can't show the fleet: what is wrong, and what happens next. */
export function ErrorState({
  title,
  icon: Glyph,
  tone = "bad",
  children,
  className,
}: {
  title: ComponentChildren;
  icon?: Icon;
  tone?: Tone;
  children?: ComponentChildren;
  className?: string;
}) {
  return (
    <div
      role="alert"
      className={cn("border-line bg-panel rounded-md border px-4 py-12 text-center", className)}
    >
      {Glyph && (
        <div className={cn("mx-auto mb-3 grid size-10 place-items-center rounded-full", toneChip(tone))}>
          <Glyph aria-hidden="true" className="size-5" />
        </div>
      )}
      <p className="m-0 text-sm font-semibold">{title}</p>
      {children && <p className="text-muted mt-1 mb-0 text-sm">{children}</p>}
    </div>
  );
}
