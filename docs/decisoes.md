# Decisões, riscos e números (v2)

Conferido em 16/10/2026. A v1 (Drive + Tailscale + DuckDNS) está na tag `v1-final`.

## Decisões

| Tema | Decisão | Por quê |
|---|---|---|
| Rede | **iroh 1.3** (crates.io), preset N0 (relays e descoberta públicos da n0). | P2P QUIC com furo de NAT e relay de reserva; o usuário não instala nada. |
| Coordenação | Cloudflare Worker + **Durable Objects SQLite** (únicos disponíveis no plano grátis). Sem D1. | CAS de verdade (um DO por mundo processa um pedido por vez). |
| Armazenamento | Padrão: o próprio DO, em 16 partes por mundo (1 GB por DO grátis; linhas de até 2 MB → pedaços de 1,9 MB). R2 opcional (exige cartão). | Sem cartão. Interface `BlobStore` permite trocar. |
| Cifra | XChaCha20-Poly1305 com a chave do mundo; a chave vai só no fragmento `#` do link. | A Cloudflare só guarda bytes opacos. |
| Autenticação | Ed25519 por app (mesma chave do iroh), assinatura de método+caminho+ts+hash do corpo. Sem contas. | Zero cadastro para os amigos. |
| Vez de hospedar | TTL 30 s, heartbeat 10 s, preferência de 15 s para a réplica mais nova, aviso (“vouch”) de membros que alcançam o host. | Failover < 90 s sem falso positivo por rede da Cloudflare. |
| Endereço | Proxy em `127.0.0.1:25565`; servidor só em `127.0.0.1:25566`. | Endereço fixo, nenhuma porta aberta, nenhuma regra de firewall. |
| Identidade/licença | Interfaces `IdentityProvider` e `License` (`core/src/license.rs`); hoje nick + chave de administração do Worker. | Trocar por login Microsoft/licença paga sem mexer no núcleo. |
| Modo offline | Só com `--features personal`. Build padrão: `online-mode=true`. | Pedido do dono para o grupo dele. |
| Distribuição | Instalador e atualizações servidos pelo Worker do dono, só para quem tem convite/é membro. | Não publicar o build pessoal em site público. |
| Quick Play | Não usado: o launcher oficial e o TLauncher não expõem `--quickPlayMultiplayer` de forma confiável. O app cria a entrada **pAuletOr 3** no `servers.dat` (com backup). | Funciona nos dois launchers. |

## Números medidos (este PC, coordenação local, servidor real 1.21.11)

| Medida | Resultado |
|---|---|
| Pausa do jogo num checkpoint | 26 ms |
| Queda do host → amigo começa a assumir | 33 s |
| Atualizar o mundo local (só a diferença, já no cache) | < 2 s numa sessão típica |
| Servidor subir (com Fabric já aquecido) | ~5–30 s |
| Integridade após failover (terceiro PC) | idêntico, hash a hash |
| Testes | 60 unitários + 10 integração (Rust), 13 do Worker, contrato Rust↔Worker local **e em produção** |

Antes do aquecimento do Fabric (`--initSettings` no preparo), a primeira subida num PC novo levava até ~80 s porque o launcher baixa bibliotecas.

## Riscos conhecidos

| Risco | Impacto | Mitigação |
|---|---|---|
| CGNAT de operadoras (furo de NAT falha) | Conexão via relay da n0 (servidores nos EUA/Europa): +100–200 ms de ping. | Saúde mostra “conexão relay”; possível relay próprio (iroh-relay é open source) no futuro. |
| Relays públicos da n0 sem SLA | Sem relay, dois PCs atrás de CGNAT não se falam. | Relay próprio como plano B. |
| Limites do plano grátis mudarem | Pedidos negados. | Uso estimado < 30% dos limites; erros viram mensagem humana. |
| Armazenamento 5 GB na conta | Mundo grande + histórico. | Cota de 4 GB por mundo, GC automático, opção R2. |
| Host sozinho perde internet por > 25 s | O servidor fecha (segurança contra dois hosts); o progresso fica salvo e é enviado depois. | Documentado; raro. |
| SmartScreen | Aviso de app desconhecido. | Página do convite e manual explicam “Executar assim mesmo”. |
| Mojang/EULA | O servidor oficial é baixado da Mojang no PC de cada um, com EULA aceito pelo usuário; nada da Mojang é redistribuído. O build pessoal (offline) é só para o grupo. | Não vender nem divulgar o build pessoal. |
