# Atualização automática do Machenia

Quem tem o app instalado vê "Atualização disponível" no sino da barra superior e no rodapé
da barra lateral. O download acontece sozinho, em segundo plano. Com um clique em
**Atualizar agora**, o app instala e reabre na versão nova, mostrando "Atualizado para a
versão X ✓" uma vez.

**Regra de ouro: o mundo nunca corre risco.** O app só se fecha para instalar quando não está
hospedando, replicando, sincronizando nem trocando de host. Para quem é host, o botão vira
**Passar o host e atualizar**: o host passa para um amigo antes, ou o mundo é salvo com o envio
à nuvem conferido. A instalação só acontece se uma destas duas coisas se confirmar em até 90 s:

1. um amigo confirmou que tem a versão final do mundo; ou
2. o envio final à nuvem terminou e foi conferido. O `push` confere cada objeto e o hash do
   manifesto, e a coordenação aponta o HEAD para o manifesto que este PC publicou.

Se nenhuma das duas se confirmar, a atualização é cancelada, o mundo fica como está (o
fechamento continua até o fim) e o usuário recebe um aviso.

## Onde está cada parte

| Parte | Arquivo |
|---|---|
| Máquina de estados (pura, com testes) | `core/src/atualizacao.rs` |
| Fechar a sessão de modo seguro para atualizar | `Engine::encerrar_para_atualizar` em `core/src/session.rs` |
| Bloqueio por versão mínima | `Engine::exigir_versao_minima`, `Status.desatualizado` |
| Serviço no app (procura, baixa, instala) | `app/src-tauri/src/atualizacao.rs` |
| Interface (sino, cartão, bloqueio) | `app/src/components/Atualizacao.tsx`, `app/src/lib/atualizacao.ts` |
| Worker: updater, canais, versão mínima | `coordinator/src/app-dist.ts` |
| Publicação | `.github/workflows/publicar-pessoal.yml`, `.github/workflows/release.yml`, `scripts/publicar-app.ps1` |

Estados: `SemAtualizacao → Disponivel → Baixando → ProntaParaInstalar → AguardandoMomentoSeguro → Instalando`.

- O app procura **ao abrir** (15 s depois) e **a cada 1 h** enquanto estiver aberto. Se der
  falha (sem internet, servidor fora), ela é silenciosa: só vai para o log, e o app tenta de
  novo na hora seguinte.
- O download começa assim que a atualização é encontrada (`Update::download`). A instalação
  (`Update::install`) só vem depois do clique.
- Um download com **assinatura errada** é recusado pelo plugin. Nada é instalado, e o cartão
  mostra "Não deu para baixar a atualização agora".
- Antes de o processo sair para instalar, o gancho `on_before_exit` confere uma última vez se
  o servidor local está parado. Se não estiver, salva e fecha.
- **Atualizar ao fechar**: no "Sair", o app faz o mesmo fluxo seguro e instala sem perguntar de
  novo.

## De onde vêm as atualizações

| Build | Fonte | Quem recebe |
|---|---|---|
| **Pessoal** (`--features personal`, modo offline) | Worker do dono, `/u/:mundo/latest.json` (só membros, com cabeçalhos assinados) | o grupo |
| **Padrão** (online-mode) | GitHub Releases `pauletor3world/pauletor3`, hoje **em rascunho** | ninguém, até a versão comercial |

O build pessoal **nunca** vai para o GitHub. O `tauri.conf.json` não tem endpoint de reserva
(`endpoints: []`): o endereço é definido só em tempo de execução, e sem endereço o app não
procura nada.

## A chave de assinatura (leia com atenção)

Cada atualização é assinada com uma chave privada (minisign). O app instalado só aceita
atualizações assinadas por ela; a chave **pública** já vai dentro do app
(`plugins.updater.pubkey` no `tauri.conf.json`).

- **Onde fica a privada:** `D:\DevTools\pauletor3-signing\pauletor3-updater.key`, com a senha
  em `senha-da-chave.txt` na mesma pasta, fora do repositório. No GitHub, ela fica só nos
  secrets `TAURI_SIGNING_PRIVATE_KEY` (conteúdo do arquivo) e
  `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`. Nunca no repositório, em logs ou em `.env` (o
  `.gitignore` bloqueia `*.key`).
- **Backup (faça hoje):**
  1. guarde o conteúdo do `.key` e a senha no gerenciador de senhas (1Password, Bitwarden etc.);
  2. faça uma cópia offline: um pendrive guardado em casa, ou o texto impresso em papel;
  3. confira de vez em quando se a cópia abre.
- **⚠️ Se a chave for perdida:** ninguém que já tem o app consegue mais receber atualização
  automática. Seria preciso mandar um instalador novo, à mão, para cada pessoa.
- **Se a chave vazar:**
  1. gere um par novo: `npx tauri signer generate -w D:\DevTools\pauletor3-signing\machencraft-updater-2.key`;
  2. publique **uma última versão assinada com a chave antiga** que já traga a pubkey nova no
     `tauri.conf.json`, para que todos migrem;
  3. troque os secrets do GitHub pela chave nova e apague a antiga de todos os lugares;
  4. troque também o `ADMIN_TOKEN` do Worker (`wrangler secret put ADMIN_TOKEN`). Só com a
     chave, um atacante não consegue servir nada: o updater do build pessoal também exige o
     Worker, e só o dono publica nele.
- **Começar do zero** (só se não houver app instalado em lugar nenhum): `npx tauri signer
  generate -w <arquivo.key>`. Ele pede uma senha. Cole o conteúdo do `.pub` em
  `plugins.updater.pubkey`.

Build local assinado (gera o `-setup.exe` e o `.sig` em `target\release\bundle\nsis`):

```powershell
$env:TAURI_SIGNING_PRIVATE_KEY = (Get-Content D:\DevTools\pauletor3-signing\pauletor3-updater.key -Raw).Trim()
$env:TAURI_SIGNING_PRIVATE_KEY_PASSWORD = (Get-Content D:\DevTools\pauletor3-signing\senha-da-chave.txt -Raw).Trim()
cd app; npx tauri build --features personal
```

## Publicar uma versão

1. Escreva a seção `## [X.Y.Z]` no `CHANGELOG.md`, em português, do ponto de vista de quem
   joga. Ela vira o texto do aviso no app e o corpo da release.
2. Troque a versão nos três arquivos de uma vez:
   `scripts\publicar-app.ps1 -Versao X.Y.Z -SemCompilar` (ou edite à mão;
   `node scripts/conferir-versao.mjs` confere).
3. Faça o commit e crie a tag: `git tag vX.Y.Z; git push origin vX.Y.Z`.
4. Dois workflows rodam:
   - **publicar build pessoal** compila com `--features personal`, assina e faz o `PUT` no
     Worker. Os apps abertos recebem em até 1 h.
   - **release** compila o build padrão e cria uma release em **rascunho** no GitHub, com o
     instalador, o `.sig` e o `latest.json`.

   Os dois falham se a versão divergir entre os arquivos ou a tag.

Secrets do repositório: `TAURI_SIGNING_PRIVATE_KEY`, `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`,
`COORDENACAO_URL` (endereço do Worker) e `ADMIN_TOKEN` (chave de administração do Worker,
a mesma de `scripts\.cache\coordenacao.txt`).

Sem GitHub Actions, dá para publicar do próprio PC: `scripts\publicar-app.ps1 -Versao X.Y.Z`.

## Versão mínima obrigatória

A troca de host só funciona com todo mundo na mesma versão. O dono define a versão mínima no
Worker, **sem publicar app novo**:

```powershell
scripts\versao-minima.ps1 -Versao 2.1.0   # define
scripts\versao-minima.ps1                 # mostra a atual
scripts\versao-minima.ps1 -Tirar          # tira
```

O valor também pode vir da variável `VERSAO_MINIMA` do Worker, que serve de reserva.

- Abaixo do mínimo, o app bloqueia Jogar, hospedar e assumir numa troca, e mostra só
  "Atualize para continuar jogando com o grupo" com o botão **Atualizar**. A coordenação
  também recusa a vez (`app_outdated`).
- Com a coordenação fora do ar, nada é bloqueado (modo degradado), e o aviso de atualização
  continua normal.
- O app informa a própria versão em cada presença e ao pedir a vez. Na troca planejada, a
  coordenação só deixa assumir quem está na mesma versão do host (`app_version_mismatch`). O
  host também só passa a vez para amigos da mesma versão: se não houver nenhum, salva e fecha
  o mundo e explica o motivo.
- Para marcar uma versão como obrigatória já na publicação: `publicar-app.ps1 -Obrigatoria`.

## Código privado: releases num repositório público separado

Se o código ficar privado, o build padrão pode publicar num repositório público só de
releases (ex.: `pauletor3world/machencraft-releases`), sem expor o código:

1. crie o repositório público vazio (só com um README);
2. crie um token *fine-grained* com **Contents: Read and write** só nesse repositório e salve
   como secret `RELEASES_TOKEN` no repositório do código;
3. no `release.yml`, troque `GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}` por
   `GITHUB_TOKEN: ${{ secrets.RELEASES_TOKEN }}` e adicione, no `with:` do `tauri-action`,
   `owner: pauletor3world`, `repo: machencraft-releases` e `releaseCommitish: main`;
4. troque `ENDPOINT_PADRAO` em `app/src-tauri/src/atualizacao.rs` para
   `https://github.com/pauletor3world/machencraft-releases/releases/latest/download/latest.json`.

O build pessoal não muda: ele continua indo só para o Worker.

## Preparado para a versão comercial (não ativado)

- **Endpoint em tempo de execução:** o `config.json` de cada PC aceita `endpoint_atualizacao`,
  que troca o endereço do `latest.json`. O app já envia cabeçalhos definidos no Rust
  (`UpdaterBuilder::header`). Para vender, o `latest.json` pode ser servido pelo Worker
  conferindo a licença num cabeçalho `Authorization`. Basta acrescentar o cabeçalho em
  `buscar()` (`app/src-tauri/src/atualizacao.rs`) e conferir a licença em `updater()`
  (`coordinator/src/app-dist.ts`).
- **Canais `estavel` e `teste`:** para testar com os amigos antes de liberar para todos:
  1. publique no canal de teste: `scripts\publicar-app.ps1 -Versao 2.2.0-beta.1 -Canal teste`
     (ou o workflow **publicar build pessoal** manual, com `canal = teste`);
  2. em cada PC que vai testar, ponha `"canal_atualizacao": "teste"` no `config.json` da pasta
     de dados (com o app fechado);
  3. quando estiver bom, publique a versão final no canal estável (o padrão).

  O Worker guarda as duas versões separadas (`?canal=teste`).

## Testes de ponta a ponta (fase 7)

Automáticos: `cargo test -p machenia-core` (máquina de estados e regras) e
`cd coordinator; npx vitest run` (versão mínima, troca entre versões, canais, notas).

Roteiro manual. O script `scripts\e2e-atualizacao.ps1` ajuda nos passos marcados com ▶.

| # | Cenário | Como | Esperado |
|---|---|---|---|
| 1 | Atualização normal | Instale a versão N. Publique N+1 no canal de teste (▶ `e2e-atualizacao.ps1 publicar-teste`) com o PC no canal de teste. | O sino ganha o selo, o cartão mostra as notas, baixa com %, **Atualizar agora** instala, o app reabre e mostra "Atualizado para a versão N+1 ✓". |
| 2 | Host com amigo online | Hospede com um amigo jogando. Anote o hash do mundo (▶ `hash-mundo`). Clique em **Passar o host e atualizar**. | Aviso no chat, o amigo assume e ninguém sai do jogo. O app instala só depois. No PC do amigo, o hash do mundo salvo bate (o progresso continua). |
| 3 | Host sozinho | Hospede sem ninguém. Clique em **Passar o host e atualizar**. | "Mundo salvo", o envio à nuvem é conferido e só então instala. Ao reabrir, o Diário mostra o salvamento. |
| 4 | Internet cai no download | Desligue o Wi-Fi/cabo no meio da barra de progresso. | Aparece "Não deu para baixar a atualização agora…" e o app segue normal. Com a internet de volta, a próxima verificação (ou abrir de novo) baixa. |
| 5 | Assinatura errada | ▶ `e2e-atualizacao.ps1 assinatura-errada` (publica no canal de teste com o `.sig` adulterado). | O download é recusado, nada é instalado e aparece o aviso. O log diz "download da atualização falhou". |
| 6 | Abaixo do mínimo | ▶ `e2e-atualizacao.ps1 minimo-acima`. | Em até 1 min, Jogar fica bloqueado e só aparece "Atualize para continuar jogando com o grupo". Volte com ▶ `minimo-tirar`. |
| 7 | Coordenação fora | Com o mínimo acima, desligue a internet. | O app não bloqueia (modo degradado). |
