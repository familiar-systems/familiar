import { Moon, Sun } from "lucide-react";
import { useTheme } from "../lib/theme";

interface ThemeToggleProps {
  className?: string;
}

export function ThemeToggle({ className }: ThemeToggleProps): React.ReactElement {
  const { theme, toggle } = useTheme();
  const isDark = theme === "dark";

  return (
    <button
      type="button"
      onClick={toggle}
      aria-label={isDark ? "Switch to light theme" : "Switch to dark theme"}
      className={[
        "inline-flex items-center justify-center rounded-full p-2",
        "border border-foreground/10 bg-background/40 backdrop-blur-md",
        "text-foreground/70 hover:text-foreground hover:bg-background/60",
        "transition-colors",
        className ?? "",
      ].join(" ")}
    >
      {isDark ? <Sun className="w-5 h-5" /> : <Moon className="w-5 h-5" />}
    </button>
  );
}
