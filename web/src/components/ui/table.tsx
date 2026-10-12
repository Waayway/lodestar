import * as React from "react"
import { cn } from "@/lib/cn"

// The machines table: lives in a bordered box that scrolls sideways on narrow screens, so
// the page itself never does. Rows are separated by dotted rules, like soundings on a chart.

function Table({
  className,
  boxClassName,
  ...props
}: React.ComponentProps<"table"> & { boxClassName?: string }) {
  return (
    <div
      data-slot="table-container"
      className={cn("border-line bg-panel relative w-full overflow-x-auto rounded-md border", boxClassName)}
    >
      <table
        data-slot="table"
        className={cn("w-full border-collapse text-left", className)}
        {...props}
      />
    </div>
  )
}

function TableHeader({ className, ...props }: React.ComponentProps<"thead">) {
  return <thead data-slot="table-header" className={className} {...props} />
}

function TableBody({ className, ...props }: React.ComponentProps<"tbody">) {
  return (
    <tbody
      data-slot="table-body"
      className={cn("[&_tr:last-child_td]:border-b-0", className)}
      {...props}
    />
  )
}

function TableRow({ className, ...props }: React.ComponentProps<"tr">) {
  return <tr data-slot="table-row" className={className} {...props} />
}

function TableHead({ className, ...props }: React.ComponentProps<"th">) {
  return (
    <th
      data-slot="table-head"
      className={cn(
        "border-line text-muted font-display tracking-label border-b px-3.5 py-2.5 text-xs font-medium whitespace-nowrap uppercase",
        className
      )}
      {...props}
    />
  )
}

function TableCell({ className, ...props }: React.ComponentProps<"td">) {
  return (
    <td
      data-slot="table-cell"
      className={cn(
        "border-line-soft border-b border-dotted px-3.5 py-3 align-middle",
        className
      )}
      {...props}
    />
  )
}

export { Table, TableHeader, TableBody, TableHead, TableRow, TableCell }
