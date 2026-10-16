# pAuletOr 3

O nosso mundo de Minecraft, hospedado por turnos. O mundo mora na nuvem (Google Drive de uma conta dedicada). Quem quer jogar clica em **Jogar**, e o PC dessa pessoa vira o servidor. Quando todo mundo sai, o mundo é salvo, enviado e a vez fica livre. Não há servidor ligado 24/7 nem custo.

![Tela inicial](docs/img/inicio.png)

- **Jogabilidade 100% vanilla:** servidor Minecraft Java **1.21.11** com Fabric Loader **0.19.5** e Lithium **0.21.4**, só no servidor. Os jogadores usam o cliente vanilla 1.21.11 (launcher oficial ou TLauncher).
- **Integridade em primeiro lugar:** sincronização endereçada por conteúdo, com o HEAD gravado por último. Um envio interrompido nunca estraga o mundo oficial.
- **Nunca dois hosts ao mesmo tempo:** trava com heartbeat, desempate determinístico e verificação antes de publicar.
- **Rede privada:** Tailscale. A porta do jogo nunca é exposta na internet. Endereço fixo via DuckDNS.
- **Diário no Discord:** a história do mundo em embeds (sessões, entradas e saídas, mortes, conquistas, marcos, resumo diário).

Documentação:

- [docs/manual-do-jogador.md](docs/manual-do-jogador.md): para quem joga.
- [docs/arquitetura.md](docs/arquitetura.md): como funciona por dentro, com diagramas.
- [docs/decisoes.md](docs/decisoes.md): o que foi verificado, as decisões e os riscos.

## Estrutura

```
core/            pauletor3-core: sincronização, trava, servidor, sessão, diário (Rust)
  src/bin/       pauletor3-cli: a mesma lógica pela linha de comando
  tests/         testes de integração (rclone com backend local)
app/             app desktop (Tauri 2 + React + TypeScript + Tailwind)
  src-tauri/     ponte Rust, bandeja, notificações, updater, bloqueio de desligamento
scripts/         rclone (sidecar), ícones, tabela de mortes do jogo
docs/            documentação
legacy/          projeto antigo (servidor em nuvem), mantido só como referência
```

## Desenvolvimento (Windows)

Pré-requisitos: Rust (stable, MSVC), Build Tools do Visual Studio com C++, Node 22. Neste PC estão em `D:\DevTools` (`RUSTUP_HOME`/`CARGO_HOME` apontam para lá).

```powershell
powershell -ExecutionPolicy Bypass -File scripts\baixar-rclone.ps1   # sidecar com SHA-256 conferido
cd app; npm ci; cd ..

cargo test -p pauletor3-core                 # unitários + integração (precisa do rclone)
cargo clippy --workspace --all-targets -- -D warnings
cd app; npm run typecheck; npm run lint; npm run build

cd app; npx tauri dev                        # app em modo desenvolvimento
```

Variáveis úteis (desenvolvimento):

- `PAULETOR3_HOME`: usa outra pasta de dados.
- `PAULETOR3_LOCAL_REMOTE=<pasta>`: usa uma pasta local como "nuvem".
- `PAULETOR3_TEST_IP=127.0.0.1`: hospeda sem Tailscale (só para testes).
- `PAULETOR3_RCLONE`: caminho do rclone usado nos testes de integração.

### CLI

```powershell
pauletor3-cli status | play | recover | timeline | restore <id> | gc [--dry-run]
pauletor3-cli import <pasta-do-mundo> | players | migrate-player <uuid> <nick>
pauletor3-cli health | diagnose | stats | pull | fingerprint <pasta-do-servidor>
```

`--home <pasta>` e `--local-remote <pasta>` funcionam em todos os comandos.

### Teste de ponta a ponta

[`scripts/e2e.sh`](scripts/e2e.sh) importa o mundo real numa "nuvem" local e depois:

1. baixa o Java e o servidor fixados;
2. sobe o servidor e faz checkpoint via RCON;
3. encerra;
4. confere que um "segundo PC" recebe o mundo com hashes idênticos.

## Google Drive: chave própria

O rclone v1.75 avisa que **o client ID compartilhado dele para o Google Drive está sendo aposentado e deixa de funcionar durante 2026**. Por isso o mundo usa um client ID próprio. É grátis e não pede cartão. Faça isso logado com a **conta dedicada ao mundo** (pauletor3@gmail.com):

1. Abra <https://console.cloud.google.com/>, aceite os termos e crie um projeto chamado `pAuletOr 3` (menu do topo → *Novo projeto*).
2. *APIs e serviços → Biblioteca*: procure **Google Drive API** e clique em **Ativar**.
3. *APIs e serviços → Tela de consentimento OAuth* (ou *Google Auth Platform → Branding*):
   - tipo **Externo**;
   - nome do app **pAuletOr 3**;
   - e-mail de suporte e de contato: pauletor3@gmail.com;
   - em **Escopos de dados**, adicione `.../auth/drive.file`;
   - em **Público-alvo**, clique em **Publicar app** para colocá-lo **Em produção**. Sem isso, o Google invalida o acesso a cada 7 dias. Com só o `drive.file`, que é um escopo não sensível, não há verificação.
4. *APIs e serviços → Credenciais → Criar credenciais → ID do cliente OAuth*, tipo **App para computador**, nome `pAuletOr 3`.
5. Copie o **ID do cliente** (termina com `.apps.googleusercontent.com`) e a **Chave secreta do cliente** (`GOCSPX-…`) e cole no assistente do app, no passo **Nuvem**.

No login, o Google pode mostrar "O Google não verificou este app". Clique em **Avançado → Acessar pAuletOr 3**: o app é seu. Os amigos não fazem nada disso, porque o convite já leva a chave.

## Publicar uma versão (atualização automática)

O app se atualiza sozinho a partir das GitHub Releases de `pauletor3world/pauletor3`, com atualizações **assinadas**. O repositório não contém nenhum segredo, então pode ser público.

Uma vez:

1. Crie o repositório `pauletor3world/pauletor3` (público) e envie este código.
2. Em *Settings → Secrets and variables → Actions*, crie:
   - `TAURI_SIGNING_PRIVATE_KEY`: conteúdo de `D:\DevTools\pauletor3-signing\pauletor3-updater.key`;
   - `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`: conteúdo de `D:\DevTools\pauletor3-signing\senha-da-chave.txt`.

   **Guarde uma cópia dessa pasta fora do PC.** Sem a chave não dá para publicar atualizações para quem já instalou.

A cada versão:

1. Suba o número em `app/src-tauri/tauri.conf.json` (`version`), em `app/package.json` e em `Cargo.toml` (`workspace.package.version`).
2. `git commit -am "versão 0.2.0"`, `git tag v0.2.0` e `git push --follow-tags`.
3. O workflow `release` gera o instalador, assina e publica o `latest.json`. Os apps instalados mostram **"Nova versão disponível"**.

Para gerar o instalador localmente, sem publicar:

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = Get-Content D:\DevTools\pauletor3-signing\pauletor3-updater.key -Raw
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = Get-Content D:\DevTools\pauletor3-signing\senha-da-chave.txt -Raw
cd app; npx tauri build    # → target\release\bundle\nsis\pAuletOr 3_<versão>_x64-setup.exe
```

**Compatibilidade:** manifestos, HEAD, trava, `versions.json` e `stats.json` têm um campo `format`. Um app mais antigo que encontra um formato maior se recusa a hospedar e pede para atualizar, em vez de arriscar o mundo.

## Segredos

Nenhum segredo vai para o repositório. Neste PC:

- tokens e chaves ficam em `secrets.bin`, protegido com DPAPI do Windows;
- a config do rclone fica criptografada pelo próprio rclone, e a senha dela está no DPAPI;
- o convite leva os segredos cifrados com Argon2id + XChaCha20-Poly1305.

## Licença

MIT. Minecraft é marca da Mojang/Microsoft. Este projeto não usa nenhum asset do jogo e não é afiliado a elas.
