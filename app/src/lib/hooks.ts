import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useRef, useState } from "react";
import { api, asHuman } from "./api";
import type { HumanError, Notice, Status, SyncProgress } from "./types";

/** Estado do mundo, atualizado em tempo real pelos eventos do núcleo. */
export function useStatus(enabled: boolean) {
  const [status, setStatus] = useState<Status | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    api.status().then((s) => alive && setStatus(s)).catch(() => {});
    api.refresh().then((s) => alive && setStatus(s)).catch(() => {});
    const un1 = listen<Status>("p3://status", (e) => setStatus(e.payload));
    const un2 = listen<SyncProgress>("p3://sync", (e) =>
      setStatus((s) => (s ? { ...s, sync: e.payload } : s)),
    );
    return () => {
      alive = false;
      un1.then((f) => f());
      un2.then((f) => f());
    };
  }, [enabled]);
  return status;
}

export interface Toast extends Notice {
  id: number;
}

export function useToasts() {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const next = useRef(1);
  const push = useCallback((t: Omit<Toast, "id" | "type">) => {
    const id = next.current++;
    setToasts((ts) => [...ts.slice(-3), { ...t, id, type: "notice" }]);
    window.setTimeout(() => setToasts((ts) => ts.filter((x) => x.id !== id)), t.level === "error" ? 9000 : 5500);
  }, []);
  const dismiss = useCallback((id: number) => setToasts((ts) => ts.filter((x) => x.id !== id)), []);
  useEffect(() => {
    const un = listen<Notice>("p3://notice", (e) => push(e.payload));
    return () => {
      un.then((f) => f());
    };
  }, [push]);
  return { toasts, push, dismiss };
}

/** Executa uma ação assíncrona com estado de carregamento e erro humano. */
export function useAction() {
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<HumanError | null>(null);
  const run = useCallback(async <T,>(key: string, fn: () => Promise<T>): Promise<T | undefined> => {
    setBusy(key);
    setError(null);
    try {
      return await fn();
    } catch (e) {
      setError(asHuman(e));
      return undefined;
    } finally {
      setBusy(null);
    }
  }, []);
  return { busy, error, setError, run };
}

/** Relógio que atualiza a cada `ms` (para "há 3 min", contagens etc.). */
export function useTick(ms = 15000) {
  const [, set] = useState(0);
  useEffect(() => {
    const t = window.setInterval(() => set((x) => x + 1), ms);
    return () => window.clearInterval(t);
  }, [ms]);
}
