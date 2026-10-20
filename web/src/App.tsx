import { ThemeProvider } from "@/components/brand/ThemeProvider";
import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ConnectionProvider } from "@/connection/ConnectionProvider";
import { useViewMode } from "@/connection/hooks";
import { useRoute } from "@/lib/route";
import { DashboardPage } from "@/pages/DashboardPage";
import { PublicServicesPage } from "@/pages/PublicServicesPage";
import { ServicesPage } from "@/pages/ServicesPage";

/** Providers and the page for the current route: everything else lives in the pages. */
export function App() {
  return (
    <ThemeProvider>
      <TooltipProvider delayDuration={200}>
        <ConnectionProvider>
          <Pages />
        </ConnectionProvider>
        <Toaster position="bottom-right" />
      </TooltipProvider>
    </ThemeProvider>
  );
}

/** Anonymous visitors get the public Services page whatever the route; everyone else the routed page. */
function Pages() {
  const route = useRoute();
  const mode = useViewMode();
  if (mode === "anonymous") return <PublicServicesPage />;
  return route === "services" ? <ServicesPage /> : <DashboardPage />;
}
