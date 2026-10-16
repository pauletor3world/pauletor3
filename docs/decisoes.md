# Decisões, verificações e riscos

Registro do que foi conferido nas fontes oficiais (em 15/10/2026), do que foi validado na prática e dos riscos que continuam abertos.

## 1. Versões fixadas

Ficam em [`core/versions.default.json`](../core/versions.default.json). Na primeira vez que o dono clica em Jogar, essas versões são publicadas como `versions.json` no remote, e esse arquivo passa a ser a fonte da verdade para todos os PCs.

| Item | Versão | Origem e verificação |
|---|---|---|
| Minecraft (servidor) | 1.21.11 | `piston-meta.mojang.com/mc/game/version_manifest_v2.json` → `javaVersion.majorVersion = 21`, `server.jar` sha1 `64bb6d76…`. `version.json` dentro do jar: `world_version` 4671. |
| Fabric Loader | 0.19.5 (estável) | `meta.fabricmc.net/v2/versions/loader/1.21.11`. O 0.20.0 existe, mas é instável. |
| Fabric installer/launcher | 1.1.2 | Jar do launcher em `meta.fabricmc.net/v2/versions/loader/1.21.11/0.19.5/1.1.2/server/jar`. A Fabric não publica hash. Baixei duas vezes, o sha256 foi idêntico (determinístico), e esse valor está fixado. |
| Lithium | 0.21.4 (`mc1.21.11-0.21.4-fabric`) | API do Modrinth (`/v2/project/lithium/version`), sha512 fixado. Não tem dependências (não exige Fabric API). |
| Java | Temurin 21 JRE `jdk-21.0.12.1+1`, Windows x64 | API do Adoptium (`/v3/assets/latest/21/hotspot`), sha256 fixado. |
| rclone | v1.75.2 | `downloads.rclone.org/v1.75.2/SHA256SUMS`, sha256 do zip fixado em `scripts/baixar-rclone.ps1`. |
| Tauri | 2.12 (+ plugins 2.x) | crates.io / npm. |

**Mundo "pAuletOr 3":** `level.dat` com `DataVersion 4671` e `Version.Name 1.21.11`, igual ao servidor. Não há conversão de versão. A importação recusa qualquer mundo com DataVersion maior que a do servidor.

## 2. Comportamentos validados na prática (não só na documentação)

- **rclone** (testado com o backend `local`):
  - `cat` e `lsjson` de algo que não existe saem com código 3 (ou 4), e `deletefile` de inexistente sai com 4. Esses códigos são tratados como "não existe".
  - Com `--use-json-log --stats-log-level NOTICE`, cada linha de estatística traz `stats.bytes`, `totalBytes`, `transfers` e `totalTransfers`. A barra de progresso do app usa esses campos.
  - **A config pode ficar criptografada sem interação** (`config encryption set` com `--password-command`), com a senha passada depois em `RCLONE_CONFIG_PASS`. O token do Google Drive nunca fica em texto puro em disco; a senha fica protegida com DPAPI.
- **Log do servidor com Fabric:** o formato é o vanilla (`[hh:mm:ss] [Server thread/INFO]: …`). O parser também aceita a variante `] (Minecraft) …`. Os **101 modelos de mensagem de morte** foram extraídos do `en_us.json` de dentro do `server.jar` 1.21.11 (script `scripts/gen-death-templates.py`), e cada um tem teste.
- **RCON do vanilla:**
  1. Escuta no `server-ip`. Não existe opção para prender o RCON em `127.0.0.1` sem prender o jogo também. Ver a decisão 3.2.
  2. **Lê um pacote por leitura do socket e derruba a conexão se dois pacotes chegarem grudados.** Descobri isso no teste com o servidor real, e o cliente agora manda um comando e espera a resposta antes do próximo. O servidor falso do teste imita esse comportamento.
- **Launcher do Fabric 1.1.x:** ignora `serverJar=` e usa `.fabric/server/<versão>-server.jar`. O app coloca ali a cópia do jar oficial já conferida e verifica esse arquivo antes de hospedar.
- **Console no Windows:** o launcher do Fabric imprime no código de página do Windows (não é UTF-8). O leitor do console converte com tolerância (`from_utf8_lossy`) e o Java é iniciado com `-Dstdout.encoding=UTF-8`. Antes disso, um único byte inválido derrubava a leitura.
- **Teste de ponta a ponta com o mundo real** (1,6 GB, 1.265 arquivos): importação, servidor Fabric + Lithium subindo com o mundo, checkpoint via RCON, encerramento, GC e um "segundo PC" baixando. Os **1.270 arquivos ficaram idênticos (hashes iguais)** entre os dois PCs.

## 3. Decisões de projeto (e por quê)

### 3.1 Trava de host: `locks/<sessão>.json` em vez de um único `lock.json`

O Google Drive **permite dois arquivos com o mesmo nome**. Se dois PCs gravassem `lock.json` ao mesmo tempo, poderiam surgir duas cópias e cada app leria uma. Por isso cada candidato grava a **própria** reivindicação com nome único e o vencedor é decidido de forma determinística: `(claimed_at, session_id)` menor ganha, todos leem os mesmos arquivos e chegam à mesma conclusão. Há uma janela de acomodação (6 s), conferência dupla depois de ativar e uma verificação da trava imediatamente antes de gravar o HEAD.

- **Limitação documentada:** se a gravação de um PC demorar mais que a janela de acomodação para aparecer na listagem do outro, os dois podem achar que venceram. A verificação antes do HEAD (`LockGuard::verify`) e a checagem de "HEAD esperado" impedem que um sobrescreva o outro. No pior caso, o progresso de um vira um manifesto solto na linha do tempo, sem perda.
- **Testes:** corrida simulada com 2 e 3 clientes (FsRemote) e com 2 clientes via rclone.

### 3.2 RCON preso ao IP do Tailscale e bloqueado no firewall

O servidor recebe `server-ip = <IP do Tailscale>`. Isso significa que **nem o jogo nem o RCON escutam na rede local ou na internet**, só na rede privada. Além disso:

- a senha do RCON é aleatória por sessão (32 caracteres);
- uma regra de firewall de **bloqueio** da porta 25575 tem precedência sobre qualquer liberação do `java.exe`;
- a porta do jogo (25565) é liberada só para `100.64.0.0/10`.

O controle do servidor fica no RCON (e não só no console) porque, se o app cair, o RCON permite parar com segurança o servidor que ficou órfão: a senha da sessão fica no `state.json` local.

### 3.3 Regras de firewall persistentes (e não criadas/removidas a cada sessão)

Criar regra exige administrador. Fazer isso a cada sessão significaria duas janelas do UAC por partida. As regras são criadas uma vez (assistente ou "Corrigir" em Saúde) e são inofensivas fora de sessão, porque só valem para tráfego vindo do Tailscale. O desinstalador tenta removê-las.

### 3.4 Espelho de staging + cache de objetos

- **Checkpoint:** `save-off` → `save-all flush` → espelho incremental para `staging/` → `save-on`. O espelho só copia o que mudou (tamanho ou mtime em **nanossegundos**; segundos não bastam, porque uma região pode ser regravada no mesmo segundo com o mesmo tamanho).
- O staging é preenchido **antes** de o servidor subir. No primeiro teste real, sem isso, a pausa do primeiro checkpoint foi de 125 s (cópia de 1,7 GB); com o pré-espelho, cada checkpoint copia só as regiões alteradas.
- Os objetos vão para `cache/objects/<sha256>` por hardlink do staging (o staging nunca é modificado no lugar: cada cópia é temp + rename). O envio sai do cache em lote (`--files-from-raw`).
- O hash de cada objeto é **reconferido antes do envio**: o nome do objeto no remote tem que ser o hash do conteúdo.

### 3.5 Recuperação de queda

Se o app abre com `state.dirty = true`:

1. para com segurança um servidor órfão (RCON com a senha da sessão);
2. remove a própria trava antiga (o id da sessão é conhecido só por este PC);
3. pega a vez em modo manutenção;
4. **só publica como HEAD** se o HEAD ainda for o desta sessão (ninguém assumiu depois) e se o `level.dat` estiver legível. Caso contrário, envia como manifesto solto ("Recuperado após queda") e o mundo oficial não é tocado.

### 3.6 Endereço fixo

O DuckDNS aponta para o IP do Tailscale de quem hospeda, atualizado ao abrir o mundo. A API (`/update?domains&token&ip`) não documenta restrição de faixa de IP. O plano B fica a um clique (Configurações ou Saúde): o app de cada jogador reescreve a entrada "pAuletOr 3" do `servers.dat` com o IP do host, só com o Minecraft fechado e com backup.

### 3.7 Jogadores com UUID "online"

O servidor roda em modo offline (UUID v3 calculado a partir do nick). O mundo pAuletOr 3 tem **3 jogadores com UUID v4** (contas com login; um deles é o dono do single-player, `d43491f3…`, com nível 30) e **9 com UUID v3**. Sem migração, os de UUID v4 entrariam zerados. A tela **Jogadores** copia inventário, posição, conquistas e estatísticas do UUID antigo para o UUID do nick, guarda o que existia antes e publica como uma versão nova ("Ajuste de jogadores"). O cálculo do UUID offline foi conferido contra o do Minecraft.

### 3.8 Diário no Discord

- Fila persistente (`journal-queue.json`) com envio em ordem, `?wait=true`, respeito a `429 retry_after` e `X-RateLimit-*`, espera crescente sem internet e no máximo 1 mensagem/s.
- Rajadas (conquistas, mortes, entradas e saídas em até 8 s) viram um embed.
- Só o app do **host** publica eventos de sessão.
- O resumo diário sai na virada do dia se alguém estiver hospedando, ou na próxima abertura do mundo, no máximo 7 dias depois. Fica marcado no `stats.json` para não repetir.
- **"Sem duplicar":** a entrada só sai da fila depois do 2xx. Se o PC cair exatamente entre o 2xx e a gravação da fila (milissegundos), aquela mensagem pode sair de novo.

### 3.9 Pasta de dados escolhida pelo usuário

O assistente sugere o disco com mais espaço livre (aqui, o D:). Só um arquivo minúsculo (`%APPDATA%\pAuletOr3\location.json`) aponta para ela.

### 3.10 Google Drive com client ID próprio e escopo `drive.file`

O `rclone config create … drive` da v1.75 avisa: *"rclone's shared Google Drive client_id is being retired and will stop working during 2026"*. Por isso o dono cria um client OAuth próprio, do tipo "App para computador" (passo a passo no README).

- O escopo é `drive.file`: o app só enxerga os arquivos que ele mesmo criou. Por ser um escopo não sensível, o app pode ficar "Em produção" sem verificação do Google, e assim o refresh token não expira a cada 7 dias como no modo de teste.
- Todos os PCs usam o mesmo client, que vai no convite, e por isso enxergam a mesma pasta.

### 3.11 Convite

`P3.` + base64url(sal | nonce | texto cifrado), com Argon2id (64 MiB, t=3) e XChaCha20-Poly1305. Senha errada ou código alterado dá erro.

**O convite contém o token do Google Drive da conta dedicada.** Quem tem o convite e a senha tem acesso total a essa conta, e por isso ela precisa ser **exclusiva do mundo**.

## 4. Riscos conhecidos

| Risco | Impacto | Mitigação |
|---|---|---|
| **DuckDNS com IP 100.x não validado ao vivo** (precisa do seu token) e roteadores com proteção contra *DNS rebinding* podem bloquear um nome público que resolve para IP privado. | Amigos não acham o servidor pelo nome. | Checklist de Saúde compara o DNS com o IP do host; "Usar IP direto" atualiza o `servers.dat` de cada um; o endereço aparece com botão copiar. |
| Corrida na trava se o Drive demorar mais de 6 s para listar um arquivo novo. | Dois hosts por instantes. | Verificação antes do HEAD e HEAD esperado: nada é sobrescrito, só vira manifesto solto. |
| `ShutdownBlockReasonCreate` com a janela escondida na bandeja: o Windows pode não mostrar o motivo se a janela estiver oculta. | Desligamento sem aviso. | O app mostra a janela ao receber `WM_QUERYENDSESSION`; mesmo que o Windows force, o último checkpoint (≤ 15 min) está na nuvem e a recuperação de queda envia o resto ao reabrir. |
| Animais domesticados de jogadores migrados continuam com o UUID antigo como dono. | Pets podem não obedecer. | Documentado; raro. |
| Token do Drive dentro do convite. | Vazamento do convite + senha = acesso à conta do mundo. | Conta dedicada; senha separada; dá para revogar em myaccount.google.com → Segurança → apps de terceiros e gerar convites novos. |
| Fabric baixa bibliotecas (`libraries/`) do maven da Fabric no primeiro start, sem hash fixado por nós. | Dependência de terceiros. | O conjunto é determinado pelo loader fixado; o próprio launcher valida o download. |
| Client ID compartilhado do rclone sendo desligado em 2026. | Sem acesso à nuvem. | Client ID próprio (decisão 3.10), obrigatório no assistente. |
| App OAuth no modo "Teste" do Google. | Token expira em 7 dias. | Publicar o app "Em produção" (só `drive.file`, sem verificação), como no passo a passo do README. |
| Auth key do Tailscale expira (máx. 90 dias no plano grátis). | Amigos novos não entram. | O checklist avisa 14 dias antes; Configurações → Convidar → Salvar chave nova. |
| Instalação do Tailscale e do VS Installer vão para o C: (instaladores oficiais). | Pouco espaço no C:. | Os dados pesados (mundo, Java, servidor, cache) ficam na pasta escolhida no D:. |
