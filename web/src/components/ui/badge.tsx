import * as React from "react"
import { cn } from "@/lib/cn"
import { type Tone, toneChip } from "@/lib/tones"

// A state on a soft wash of its own colour (lib/tones). StatusBadge picks the tone, icon and
// text for an apply.
function Badge({
  className,
  tone = "neutral",
  ...props
}: React.ComponentProps<"span"> & { tone?: Tone }) {
  return (
    <span
      data-slot="badge"
      data-tone={tone}
      className={cn(
        "inline-flex w-fit max-w-full shrink-0 items-center gap-1.5 rounded-sm px-[9px] py-1 text-[12.5px] font-semibold whitespace-nowrap [&>svg]:pointer-events-none [&>svg]:size-4 [&>svg]:shrink-0",
        toneChip(tone),
        className
      )}
      {...props}
    />
  )
}

export { Badge }
