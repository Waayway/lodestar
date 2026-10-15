import { ThemeProvider } from "@/components/brand/ThemeProvider";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ConnectionProvider } from "@/connection/ConnectionProvider";
import { useRoute } from "@/lib/route";
import { DashboardPage } from "@/pages/DashboardPage";
import { ServicesPage } from "@/pages/ServicesPage";

/** Providers and the page for the current route: everything else lives in the pages. */
export function App() {
  const route = useRoute();
  return (
    <ThemeProvider>
      <TooltipProvider delayDuration={200}>
        <ConnectionProvider>
          {route === "services" ? <ServicesPage /> : <DashboardPage />}
        </ConnectionProvider>
        <Toaster position="bottom-right" />
      </TooltipProvider>
    </ThemeProvider>
  );
}
