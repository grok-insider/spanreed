import * as React from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { isTauri } from "@tauri-apps/api/core";

export type ThemePreference = "system" | "light" | "dark";
const THEME_KEY = "spanreed.theme";

export function useThemePreference(onError: (message: string) => void) {
  const [theme, setTheme] = React.useState<ThemePreference>(() => {
    const saved = localStorage.getItem(THEME_KEY);
    return saved === "light" || saved === "dark" ? saved : "system";
  });
  const reportError = React.useRef(onError);
  reportError.current = onError;
  React.useEffect(() => {
    const media = matchMedia("(prefers-color-scheme: dark)");
    const apply = () => {
      const dark = theme === "dark" || (theme === "system" && media.matches);
      document.documentElement.classList.toggle("dark", dark);
      document.documentElement.dataset.gem = "ruby";
      document.documentElement.style.colorScheme = dark ? "dark" : "light";
      if (isTauri()) void getCurrentWindow().setTheme(theme === "system" ? null : dark ? "dark" : "light")
        .catch((error) => reportError.current(`Could not update window appearance: ${String(error)}`));
    };
    apply();
    media.addEventListener("change", apply);
    localStorage.setItem(THEME_KEY, theme);
    return () => media.removeEventListener("change", apply);
  }, [theme]);
  return [theme, setTheme] as const;
}
