# Arquitetura (v2)

```mermaid
flowchart LR
  subgraph PC1["PC de quem hospeda"]
    MC1["Minecraft"] -->|"localhost:25565"| PX1["proxy local"]
    PX1 --> SRV["servidor Fabric 1.21.11 + Lithium<br/>127.0.0.1:25566 · RCON 127.0.0.1"]
    CORE1["pauletor3-core<br/>(sessão, sync, lease)"] --> SRV
  end
  subgraph PC2["PC de um amigo"]
    MC2["Minecraft"] -->|"localhost:25565"| PX2["proxy local"]
  end
  PX2 == "túnel iroh (QUIC P2P;<br/>relay n0 se o furo de NAT falhar)" ==> PX1
  CORE1 <-. "réplica P2P (objetos por hash)" .-> PX2
  CORE1 -->|"HTTPS assinado (Ed25519)"| W["Worker Cloudflare"]
  PX2 -->|"presença a cada 4–20 s"| W
  W --> DO1[("WorldDO<br/>lease · HEAD · membros · convites · diário")]
  W --> DO2[("BlobShardDO ×16<br/>mundo cifrado")]
```

## Peças

| Peça | O que faz |
|---|---|
| `core/src/coord` | Contrato da coordenação (`Coordinator`), regras puras (`rules.rs`), implementação em memória (testes) e cliente HTTP. |
| `core/src/lease.rs` | Posse da vez: heartbeat a cada 10 s, detecção de vez perdida e de isolamento. |
| `core/src/sync.rs` | Modelo endereçado por conteúdo (objetos sha256 + manifestos). HEAD só muda por CAS na coordenação, por último. Busca objetos nos amigos (P2P) antes da nuvem. |
| `core/src/p2p.rs` | Nó iroh: ALPN `pauletor3/mc/1` (túnel do jogo) e `pauletor3/sync/1` (manifestos, objetos, ping). Só membros conectam. |
| `core/src/proxy.rs` | Proxy em `127.0.0.1`: lê o handshake do Minecraft e encaminha (local, túnel) ou responde o MOTD/aviso e acorda o mundo. |
| `core/src/remote/crypt.rs` | Cifra XChaCha20-Poly1305 com a chave do mundo (viaja só no `#` do convite). |
| `core/src/session.rs` | Orquestra jogar, checkpoints, fim de sessão, recuperação, failover, réplica em segundo plano. |
| `coordinator/` | Worker + Durable Objects (SQLite). Um `WorldDO` por mundo serializa as operações (CAS de verdade). |

## Sessão e failover

```mermaid
sequenceDiagram
  autonumber
  participant B as App do amigo (jogando)
  participant W as Coordenação
  participant A as App do host
  A->>W: acquire (session_number = n)
  loop a cada 10 s
    A->>W: heartbeat(n)
  end
  loop a cada 1–5 min
    A->>A: save-off · save-all flush · espelho · save-on
    A->>W: objetos + manifesto (cifrados)
    A->>W: HEAD CAS (n, esperado) → novo HEAD
    B->>A: réplica P2P do que mudou
  end
  Note over A: queda de energia
  B--xA: ping falha (4 s)
  Note over W: 30 s sem heartbeat nem aviso de membro → vez expira
  B->>W: acquire (preferência para a réplica mais nova por 15 s)
  W-->>B: session_number = n+1
  B->>B: monta o HEAD (já está no cache) e sobe o servidor
  Note over B: proxy dos amigos aponta para B
  A->>W: (se voltar) HEAD CAS com n → lost_lock: vira manifesto solto
```

## Regras de segurança do mundo

- **Nunca dois hosts:** a coordenação concede uma vez por vez (DO serializado). O host que perde contato por 25 s sem amigos conectados se encerra sozinho; com amigos conectados, eles “avisam” que o host vive e a vez não expira.
- **Réplica velha nunca vence:** publicar exige o HEAD esperado e o `session_number` atual.
- **Nada corrompe o oficial:** objetos conferidos por hash ao receber; HEAD gravado por último; montagem com troca atômica.

## Custos (plano grátis da Cloudflare)

Presença devolve a fotografia do mundo (1 pedido por ciclo); presença só em memória; heartbeat no disco no máximo a cada 15 s. Estimativa para 3–5 amigos: < 30 mil pedidos/dia (limite 100 mil) e < 20 mil linhas gravadas/dia (limite 100 mil). Armazenamento: até 4 GB por mundo (limite da conta: 5 GB).
