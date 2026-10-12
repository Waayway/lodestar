import { Toaster as Sonner, type ToasterProps } from "sonner"
import {
  ToastError,
  ToastInfo,
  ToastSuccess,
  ToastWarning,
  Building,
} from "@/icons"
import { useTheme } from "@/lib/theme"

/** Toasts in the chart's panel colours, following the theme the header toggle resolved. */
const Toaster = ({ ...props }: ToasterProps) => {
  const { resolved } = useTheme()
  return (
    <Sonner
      theme={resolved}
      className="toaster group"
      icons={{
        success: <ToastSuccess className="text-ok size-4" />,
        info: <ToastInfo className="text-info size-4" />,
        warning: <ToastWarning className="text-warn size-4" />,
        error: <ToastError className="text-bad size-4" />,
        loading: <Building className="size-4 animate-spin" />,
      }}
      style={
        {
          "--normal-bg": "var(--panel)",
          "--normal-text": "var(--fg)",
          "--normal-border": "var(--line)",
          "--border-radius": "var(--r)",
          fontFamily: "var(--font)",
        } as React.CSSProperties
      }
      {...props}
    />
  )
}

export { Toaster }
