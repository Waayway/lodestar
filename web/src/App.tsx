import { Toaster } from "@/components/ui/sonner";
import { TooltipProvider } from "@/components/ui/tooltip";
import { ConnectionProvider } from "@/connection/ConnectionProvider";
import { DashboardPage } from "@/pages/DashboardPage";

/** Providers only: everything else lives in the page and its components. */
export function App() {
  return (
    <TooltipProvider delayDuration={200}>
      <ConnectionProvider>
        <DashboardPage />
      </ConnectionProvider>
      <Toaster position="bottom-right" />
    </TooltipProvider>
  );
}
