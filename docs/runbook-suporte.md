# Suporte: problemas comuns

| Sintoma | Causa provável | O que fazer |
|---|---|---|
| “Sem conexão com a nuvem do mundo” | Internet caiu, ou relógio do Windows muito errado (assinatura recusada). | Conferir internet; acertar data/hora automática do Windows; Saúde → Tentar de novo. |
| Amigo fica em “Entrando...” | O mundo está abrindo ou trocando de host (sala de espera do proxy). | Esperar: entra sozinho. Depois de 5 min o proxy desiste e mostra o motivo. |
| Saúde mostra “conexão relay” | NAT/CGNAT impede conexão direta. | Funciona, com ping maior. Reiniciar o roteador às vezes resolve. |
| “A porta 25565 já está em uso” | Outro servidor de Minecraft aberto no PC. | Fechar o outro servidor, ou Configurações → Porta no Minecraft (ex.: 25570) e reiniciar o app. |
| Amigo “aguardando aprovação” | Convite com aprovação. | Dono: Grupo → Pedidos para entrar → Aprovar. |
| Convite “não vale mais” | Expirou (48 h) ou usos acabaram. | Dono gera outro em Grupo. |
| Sessão interrompida neste PC | O app/PC caiu hospedando. | Abrir o app: a recuperação roda sozinha (ou Saúde → Recuperar agora). Nada sobrescreve o mundo de quem assumiu. |
| “A nuvem do mundo está cheia” | Cota de 4 GB. | Saúde → Liberar espaço (GC); ou migrar para R2. |
| Entrou sem inventário | Nick diferente do usado antes. | Dono: Jogadores → vincular o UUID antigo ao nick. |

Diagnóstico: Saúde → **Copiar diagnóstico** (sem segredos). Logs: `<pasta de dados>\logs`.

## Operação da coordenação

- Atualizar o Worker: `cd coordinator; npx wrangler deploy`.
- Logs ao vivo: `npx wrangler tail`.
- Nova chave de administração: apagar `scripts\.cache\coordenacao.txt` e rodar `scripts\publicar-coordenacao.ps1`.
- Publicar versão do app: `scripts\publicar-app.ps1 -Versao X.Y.Z`.
