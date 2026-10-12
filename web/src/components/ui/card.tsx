import * as React from "react"
import { cn } from "@/lib/cn"

/** A panel on the chart: hairline border, panel fill, 16 px inside, children 10 px apart. */
function Card({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card"
      className={cn(
        "border-line bg-panel flex min-w-0 flex-col gap-2.5 rounded-md border p-4",
        className
      )}
      {...props}
    />
  )
}

/** A card's first row: title on the left, a note or badge on the right. */
function CardHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="card-header"
      className={cn("flex items-center justify-between gap-2.5", className)}
      {...props}
    />
  )
}

export { Card, CardHeader }
