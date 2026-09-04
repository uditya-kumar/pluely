import { createContext, useContext, useEffect, useState } from "react";
import { emit, listen } from "@tauri-apps/api/event";
import { STORAGE_KEYS } from "@/config/";

type Theme = "dark" | "light" | "system";
type ResolvedTheme = "dark" | "light";

// Broadcast so the overlay window and the dashboard window stay in sync
// immediately, without waiting for a storage event.
const THEME_SYNC_EVENT = "pluely-theme-sync";

type ThemeSyncPayload = {
  theme?: Theme;
  transparency?: number;
};

type ThemeProviderProps = {
  children: React.ReactNode;
  defaultTheme?: Theme;
  storageKey?: string;
};

type ThemeProviderState = {
  theme: Theme;
  resolvedTheme: ResolvedTheme;
  setTheme: (theme: Theme) => void;
  transparency: number;
  onSetTransparency: (transparency: number) => void;
};

const initialState: ThemeProviderState = {
  theme: "system",
  resolvedTheme: "dark",
  setTheme: () => null,
  transparency: 10,
  onSetTransparency: () => null,
};

const ThemeProviderContext = createContext<ThemeProviderState>(initialState);

const systemPrefersDark = () =>
  window.matchMedia("(prefers-color-scheme: dark)").matches;

export function ThemeProvider({
  children,
  defaultTheme = "system",
  storageKey = STORAGE_KEYS.THEME,
  ...props
}: ThemeProviderProps) {
  const [theme, setTheme] = useState<Theme>(
    () => (localStorage.getItem(storageKey) as Theme) || defaultTheme
  );
  const [isSystemDark, setIsSystemDark] = useState<boolean>(systemPrefersDark);
  const [transparency, setTransparency] = useState<number>(() => {
    const stored = localStorage.getItem(STORAGE_KEYS.TRANSPARENCY);
    return stored ? parseInt(stored, 10) : 10;
  });

  const resolvedTheme: ResolvedTheme =
    theme === "system" ? (isSystemDark ? "dark" : "light") : theme;

  useEffect(() => {
    const handleStorageChange = (e: StorageEvent) => {
      if (e.key === STORAGE_KEYS.TRANSPARENCY && e.newValue) {
        setTransparency(parseInt(e.newValue, 10));
      }
      if (e.key === storageKey && e.newValue) {
        setTheme(e.newValue as Theme);
      }
    };

    window.addEventListener("storage", handleStorageChange);

    const unlisten = listen<ThemeSyncPayload>(THEME_SYNC_EVENT, ({ payload }) => {
      if (payload?.theme) {
        setTheme(payload.theme);
      }
      if (typeof payload?.transparency === "number") {
        setTransparency(payload.transparency);
      }
    });

    return () => {
      window.removeEventListener("storage", handleStorageChange);
      unlisten.then((stop) => stop()).catch(() => {});
    };
  }, [storageKey]);

  // Follow the OS preference while the theme is set to "system"
  useEffect(() => {
    const mediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = (e: MediaQueryListEvent) => setIsSystemDark(e.matches);

    mediaQuery.addEventListener("change", onChange);
    return () => mediaQuery.removeEventListener("change", onChange);
  }, []);

  useEffect(() => {
    const root = window.document.documentElement;

    root.classList.remove("light", "dark");
    root.classList.add(resolvedTheme);
  }, [resolvedTheme]);

  // Apply transparency globally
  useEffect(() => {
    const root = window.document.documentElement;
    const opacity = (100 - transparency) / 100;

    // Apply opacity to CSS variables
    root.style.setProperty("--opacity", opacity.toString());

    // Apply backdrop filter when transparency is active
    if (transparency > 0) {
      root.style.setProperty("--backdrop-blur", "blur(12px)");
    } else {
      root.style.setProperty("--backdrop-blur", "none");
    }
  }, [transparency]);

  const value: ThemeProviderState = {
    theme,
    resolvedTheme,
    setTheme: (newTheme: Theme) => {
      localStorage.setItem(storageKey, newTheme);
      setTheme(newTheme);
      emit(THEME_SYNC_EVENT, { theme: newTheme } satisfies ThemeSyncPayload).catch(
        () => {}
      );
    },
    transparency,
    onSetTransparency: (newTransparency: number) => {
      localStorage.setItem(STORAGE_KEYS.TRANSPARENCY, newTransparency.toString());
      setTransparency(newTransparency);
      emit(THEME_SYNC_EVENT, {
        transparency: newTransparency,
      } satisfies ThemeSyncPayload).catch(() => {});
    },
  };

  return (
    <ThemeProviderContext.Provider {...props} value={value}>
      {children}
    </ThemeProviderContext.Provider>
  );
}

export const useTheme = () => {
  const context = useContext(ThemeProviderContext);

  if (context === undefined)
    throw new Error("useTheme must be used within a ThemeProvider");

  return context;
};
