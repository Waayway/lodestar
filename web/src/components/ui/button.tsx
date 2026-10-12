import * as React from "react"
import { cva, type VariantProps } from "class-variance-authority"
import { cn } from "@/lib/cn"
import { Slot } from "radix-ui"

// The chart's buttons: a solid ink button for the action, an outlined one beside it, and a
// borderless one for quiet controls. Square-ish corners (--r-sm), 600 weight.
const buttonVariants = cva(
  "inline-flex shrink-0 cursor-pointer items-center justify-center gap-2 rounded-sm border font-semibold whitespace-nowrap transition-colors disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg]:shrink-0 [&_svg:not([class*='size-'])]:size-4",
  {
    variants: {
      variant: {
        default: "border-btn-line bg-btn-bg text-btn-fg hover:opacity-90",
        outline: "border-line bg-transparent text-fg hover:bg-chip",
        ghost: "border-transparent bg-transparent text-fg hover:bg-chip",
      },
      size: {
        default: "min-h-10 px-3.5 text-[13px]",
        sm: "min-h-[34px] px-2.5 text-[12.5px]",
        icon: "size-10",
        "icon-sm": "size-[34px]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  }
)

// forwardRef (not React 19's ref-as-prop): under preact/compat a plain function component
// never sees `ref`, and Radix needs this node (Presence measures it, Slot focuses it).
const Button = React.forwardRef<
  HTMLButtonElement,
  React.ComponentProps<"button"> &
    VariantProps<typeof buttonVariants> & {
      asChild?: boolean
    }
>(function Button(
  { className, variant = "default", size = "default", asChild = false, type = "button", ...props },
  ref
) {
  // Typed as "button": under preact/compat the Slot/button union doesn't unify its ref types.
  const Comp = (asChild ? Slot.Root : "button") as "button"

  return (
    <Comp
      ref={ref}
      type={asChild ? undefined : type}
      data-slot="button"
      data-variant={variant}
      data-size={size}
      className={cn(buttonVariants({ variant, size, className }))}
      {...props}
    />
  )
})

export { Button, buttonVariants }
