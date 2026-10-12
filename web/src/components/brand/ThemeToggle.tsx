import { ThemeDark, ThemeLight, ThemeSystem } from "@/icons";
import { cn } from "@/lib/cn";
import { THEME_CHOICES, type ThemeChoice, useTheme } from "@/lib/theme";

const options: Record<ThemeChoice, { label: string; Icon: typeof ThemeLight }> = {
  system: { label: "Use the system theme", Icon: ThemeSystem },
  light: { label: "Light theme", Icon: ThemeLight },
  dark: { label: "Dark theme", Icon: ThemeDark },
};

/** Three small icon buttons: follow the system, or force light or dark. */
export function ThemeToggle({ className }: { className?: string }) {
  const { choice, setChoice } = useTheme();
  return (
    <div
      role="group"
      aria-label="Theme"
      className={cn("border-line bg-panel inline-flex items-center rounded-full border p-0.5", className)}
    >
      {THEME_CHOICES.map((c) => {
        const { label, Icon } = options[c];
        const active = c === choice;
        return (
          <button
            key={c}
            type="button"
            aria-label={label}
            aria-pressed={active}
            title={label}
            onClick={() => setChoice(c)}
            className={cn(
              "grid size-7 cursor-pointer place-items-center rounded-full transition-colors",
              active ? "bg-chip text-fg" : "text-muted hover:text-fg",
            )}
          >
            <Icon className="size-3.5" aria-hidden="true" />
          </button>
        );
      })}
    </div>
  );
}
