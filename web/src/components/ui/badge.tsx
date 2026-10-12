import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/cn"

// A state on a soft wash of its own colour. The tone names match the state system in
// docs/style-guide.md; StatusBadge picks the tone, icon and text for an apply.
const badgeVariants = cva(
  "inline-flex w-fit max-w-full shrink-0 items-center gap-1.5 rounded-sm px-[9px] py-1 text-[12.5px] font-semibold whitespace-nowrap [&>svg]:pointer-events-none [&>svg]:size-4 [&>svg]:shrink-0",
  {
    variants: {
      tone: {
        ok: "bg-ok-soft text-ok",
        running: "bg-info-soft text-info",
        skipped: "bg-warn-soft text-warn",
        failed: "bg-bad-soft text-bad",
        neutral: "bg-chip text-muted",
        accent: "bg-accent-soft text-accent",
      },
    },
    defaultVariants: {
      tone: "neutral",
    },
  }
)

export type BadgeTone = NonNullable<VariantProps<typeof badgeVariants>["tone"]>

function Badge({
  className,
  tone = "neutral",
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return (
    <span
      data-slot="badge"
      data-tone={tone}
      className={cn(badgeVariants({ tone }), className)}
      {...props}
    />
  )
}

export { Badge, badgeVariants }
