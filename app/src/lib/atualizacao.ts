import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useCallback, useEffect, useState } from "react";

export type EstadoAtualizacao =
  | "sem_atualizacao"
  | "disponivel"
  | "baixando"
  | "pronta_para_instalar"
  | "aguardando_momento_seguro"
  | "instalando";

export interface Atualizacao {
  estado: EstadoAtualizacao;
  versao_atual: string;
  oferta: { versao: string; notas: string | null; obrigatoria: boolean } | null;
  pedido: "agora" | "ao_fechar" | null;
  baixado: number;
  total: number | null;
  erro: string | null;
  passando_host: boolean;
  obrigatoria: boolean;
  sou_host: boolean;
  bloqueado: boolean;
  versao_minima: string | null;
  acabou_de_atualizar: string | null;
}

export interface Progresso {
  baixado: number;
  total: number | null;
}

export const atualizacaoApi = {
  estado: () => invoke<Atualizacao>("atualizacao_estado"),
  atualizar: (modo: "agora" | "ao_fechar") => invoke<Atualizacao>("atualizacao_atualizar", { modo }),
  cancelar: () => invoke<Atualizacao>("atualizacao_cancelar"),
  verificar: () => invoke<Atualizacao>("atualizacao_verificar"),
  visto: () => invoke<void>("atualizacao_visto"),
};

/** Estado da atualização do app, com o progresso do download em tempo real. */
export function useAtualizacao(enabled: boolean) {
  const [a, setA] = useState<Atualizacao | null>(null);
  const [prog, setProg] = useState<Progresso | null>(null);
  useEffect(() => {
    if (!enabled) return;
    let alive = true;
    atualizacaoApi.estado().then((v) => alive && setA(v)).catch(() => {});
    const un = listen<Atualizacao>("p3://atualizacao", (e) => setA(e.payload));
    const canal = new Channel<Progresso>();
    canal.onmessage = (p) => alive && setProg(p);
    invoke("atualizacao_progresso", { canal }).catch(() => {});
    return () => {
      alive = false;
      un.then((f) => f());
    };
  }, [enabled]);
  const atualizar = useCallback((modo: "agora" | "ao_fechar") => atualizacaoApi.atualizar(modo).then(setA), []);
  const cancelar = useCallback(() => atualizacaoApi.cancelar().then(setA), []);
  // o canal manda mais rápido que o evento de estado
  const baixado = prog?.baixado ?? a?.baixado ?? 0;
  const total = prog?.total ?? a?.total ?? null;
  return { a, baixado, total, atualizar, cancelar };
}

/** Há algo para mostrar no selo do sino / rodapé. */
export function temAtualizacao(a: Atualizacao | null): boolean {
  return !!a?.oferta && a.estado !== "sem_atualizacao";
}
