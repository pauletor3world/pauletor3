import { useCallback, useEffect, useState } from "react";

export type Theme = "light" | "dark";
const KEY = "machencraft.tema";

function inicial(): Theme {
  const dev = import.meta.env.DEV ? new URLSearchParams(location.search).get("tema") : null;
  if (dev === "claro") return "light";
  if (dev === "escuro") return "dark";
  try {
    const t = localStorage.getItem(KEY);
    if (t === "light" || t === "dark") return t;
  } catch {
    // sem armazenamento: segue o sistema
  }
  return window.matchMedia?.("(prefers-color-scheme: light)").matches ? "light" : "dark";
}

/** Tema claro/escuro: segue o Windows até a pessoa escolher no botão da barra superior. */
export function useTheme() {
  const [theme, set] = useState<Theme>(inicial);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
  }, [theme]);
  const setTheme = useCallback((t: Theme) => {
    set(t);
    try {
      localStorage.setItem(KEY, t);
    } catch {
      // só uma preferência de tela
    }
  }, []);
  return { theme, setTheme };
}
