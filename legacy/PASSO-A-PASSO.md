# Passo a passo do zero: servidor Minecraft no Google Cloud

Siga na ordem. Cada parte termina com um ✅ dizendo como saber que deu certo.

> As telas do Google Cloud e do Tailscale mudam de vez em quando, e podem aparecer em português ou inglês. Se um botão tiver um nome um pouco diferente, procure o mais parecido: o fluxo é o mesmo.

Em todos os comandos abaixo, troque `machen` pelo nome de usuário que você quiser: letras minúsculas, sem espaço nem acento.

---

## Parte 1: Ativar o free trial do Google Cloud

1. Acesse **https://console.cloud.google.com** e entre com a sua conta Google.
2. Clique em **Ativar avaliação gratuita** (ou **Comece gratuitamente**, ou **Start free**).
3. País: **Brasil**. Aceite os termos e clique em **Continuar**.
4. Tipo de conta: **Individual**.
5. Informe um **cartão de crédito**. O Google usa o cartão só para verificar a identidade. Durante o trial, ele **não cobra nada**, a menos que você mesmo faça upgrade para conta paga.
6. Clique em **Iniciar avaliação gratuita**.

✅ Aparece um aviso de crédito de **US$ 300** válido por **90 dias**.

---

## Parte 2: Criar o projeto

1. No topo da tela, ao lado do logo do Google Cloud, clique no **seletor de projeto** e depois em **Novo projeto**.
2. Nome do projeto: `minecraft`. Clique em **Criar**.
3. Quando terminar, clique no seletor de novo e escolha o projeto **minecraft**.

✅ O nome **minecraft** aparece no seletor de projeto, no topo.

---

## Parte 3: Tailscale no seu PC

1. Acesse **https://tailscale.com**, clique em **Get started** e entre com a sua conta Google. Se perguntar o tipo de uso, escolha **Personal use**.
2. Instale o app para Windows: **https://tailscale.com/download/windows**.
3. Clique no ícone do Tailscale perto do relógio, depois em **Log in**, e entre com a **mesma conta**.

✅ Seu PC aparece em **https://login.tailscale.com/admin/machines**.

---

## Parte 4: Criar a chave SSH (no seu PC)

1. Abra a pasta do projeto no **Explorador de Arquivos**.
2. Clique na **barra de endereço**, digite `powershell` e aperte **Enter**.
3. Rode:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\windows\new-key.ps1 -User machen
   ```

4. O script mostra uma linha começando com `ssh-ed25519 ...` e terminando com `machen`, e já copia essa linha para a área de transferência. **Deixe essa janela aberta.**

✅ Existe o arquivo `C:\Users\vlmac\.ssh\gcp-mc`. Ele é a sua chave privada: não mande para ninguém.

---

## Parte 5: Criar a VM

1. Menu **☰** → **Compute Engine** → **Instâncias de VM**.
2. Na primeira vez, aparece o botão **Ativar** (para a API do Compute Engine). Clique nele e espere de 1 a 2 minutos.
3. Clique em **Criar instância**.

### 5.1 Configuração da máquina
- **Nome:** `minecraft`
- **Região:** `southamerica-east1 (São Paulo)`
- **Zona:** qualquer uma, por exemplo `southamerica-east1-b`
- **Família:** Uso geral. **Série:** **E2**
- **Tipo de máquina:** **e2-standard-2** (2 vCPU, 8 GB de memória)

  Use a **e2-standard-2**, e não a n2d. Ela custa cerca de US$ 0,10/h, o que dá uns US$ 230 em 90 dias e cabe nos US$ 300. A n2d é mais cara e pode estourar o crédito.

### 5.2 SO e armazenamento
1. Clique em **Alterar**.
2. **Sistema operacional:** `Ubuntu`.
3. **Versão:** **Ubuntu 24.04 LTS**, a opção **x86/64** (amd64). Não use a "Minimal" nem a "Arm64".
4. **Tipo de disco de inicialização:** Disco permanente equilibrado (Balanced).
5. **Tamanho:** `30` GB.
6. Clique em **Selecionar**.

### 5.3 Outras seções
- **Proteção de dados / Backups:** se houver backup ou snapshot automático marcado, escolha **Sem backups**. Isso gasta crédito, e o servidor já faz backup próprio.
- **Rede:** **não** marque "Permitir tráfego HTTP/HTTPS". Deixe o resto como está.
- **Observabilidade:** se "Instalar o Ops Agent" estiver marcado, desmarque.

### 5.4 Criar
1. Clique em **Criar** e espere a bolinha ficar verde.
2. Na lista de instâncias, **anote o "IP externo"**, por exemplo `34.95.10.20`.

✅ A VM `minecraft` aparece com a bolinha verde e um IP externo.

---

## Parte 6: Colar a chave na VM

1. Na lista de instâncias, clique no nome **minecraft**.
2. No topo, clique em **Editar**.
3. Desça até **Segurança e acesso** e encontre **Chaves SSH**.
4. Clique em **Adicionar item** e cole com **Ctrl+V** a linha que o `new-key.ps1` mostrou (`ssh-ed25519 ... machen`).
5. No fim da página, clique em **Salvar**.

✅ Ao lado da chave aparece o usuário **machen**.

### Conferir o OS Login
Se o OS Login estiver ligado, o Google ignora a chave que você colou, e o deploy dá `Permission denied (publickey)`. Para conferir:
1. Menu **☰** → **Compute Engine** → **Metadados** (em Configurações).
2. Veja se existe a chave `enable-oslogin` com valor `TRUE`.
   - **Não existe, ou está `FALSE`:** tudo certo, siga em frente.
   - **Está `TRUE`:** vá em **VM minecraft → Editar → Metadados → Adicionar item**, coloque a chave `enable-oslogin` com valor `FALSE` e clique em **Salvar**.

---

## Parte 7: Instalar o servidor (um comando)

No PowerShell da pasta do projeto (o mesmo da Parte 4), rode trocando o IP pelo da Parte 5.4:

```powershell
powershell -ExecutionPolicy Bypass -File .\windows\deploy.ps1 -Ip 34.95.10.20
```

1. Na primeira conexão, o script grava a "impressão digital" da VM sozinho.
2. Espere de 5 a 10 minutos. Vão aparecer as etapas de `1/12` a `12/12`.
3. Na etapa **11/12 Tailscale**, aparece uma caixa amarela com um link `https://login.tailscale.com/a/...`. Abra o link no navegador, entre com a **mesma conta do Tailscale** e clique em **Connect**.
4. No fim aparece o **RESUMO**, com `Serviço: active` e o endereço `100.x.y.z:25565`.

Se o resumo não mostrar o IP do Tailscale, rode:

```powershell
powershell -ExecutionPolicy Bypass -File .\windows\ssh.ps1 mc status
```

✅ A máquina `minecraft` aparece em **https://login.tailscale.com/admin/machines**.

---

## Parte 8: Deixar o Tailscale da VM permanente

Sem isso, a VM se desconecta da sua rede Tailscale depois de 180 dias.

1. Acesse **https://login.tailscale.com/admin/machines**.
2. Na linha da máquina **minecraft**, clique em **…**.
3. Clique em **Disable key expiry**.

---

## Parte 9: Enviar o seu mundo

Se você vai começar um mundo novo, pule esta parte.

1. **Feche o Minecraft.**
2. Encontre a pasta do mundo, a que tem o arquivo `level.dat` dentro. No TLauncher e no launcher oficial, ela fica em `%APPDATA%\.minecraft\saves\NOME_DO_MUNDO`. Para abrir, cole `%APPDATA%\.minecraft\saves` na barra do Explorador.
3. Rode, trocando o nome do mundo e mantendo as aspas:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\windows\push-world.ps1 -World "$env:APPDATA\.minecraft\saves\NOME_DO_MUNDO"
   ```

4. O script compacta, envia e troca o mundo na VM. O mundo que estava lá ganha um backup automático.
5. No fim, ele lista os jogadores salvos no mundo (os UUIDs).

Pontos importantes:
- O mundo precisa ser da **1.21.11 ou mais antigo**. Mais novo é recusado; mais antigo é convertido. Guarde uma cópia da pasta original, por garantia.
- **Inventários:** no servidor, cada nick tem um UUID próprio. Se você ou um amigo entrar e o inventário não estiver lá, rode:

  ```powershell
  powershell -ExecutionPolicy Bypass -File .\windows\ssh.ps1 mc player-move UUID_ANTIGO NICK
  ```

  Use um dos UUIDs da lista do passo 5. Os arquivos originais são mantidos.

✅ Você entra e vê o seu mundo.

---

## Parte 10: Entrar no jogo

1. Confira se o Tailscale do seu PC está **Connected**.
2. No TLauncher, escolha **Release 1.21.11**. O cliente é vanilla, sem mods.
3. Vá em **Multijogador** → **Adicionar servidor** → endereço `100.x.y.z` (o IP do Tailscale da VM).
4. Para virar admin (op):

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\windows\ssh.ps1 mc op SEU_NICK
   ```

---

## Parte 11: Convidar os amigos

### Você (para cada amigo)
1. Acesse **https://login.tailscale.com/admin/machines**.
2. Na linha da máquina **minecraft**, clique em **…** → **Share…**.
3. Copie o link de convite e mande para o amigo. Gere um link por amigo.

### O amigo
1. Instala o Tailscale (**https://tailscale.com/download**) e cria uma conta própria.
2. Abre o seu link e clica em **Accept**.
3. Deixa o Tailscale **Connected**.
4. No TLauncher, escolhe **1.21.11** → **Multijogador** → **Adicionar servidor** → `100.x.y.z`.

⚠️ Avise todos: **usem sempre o mesmo nick**. O inventário fica preso ao nick.

---

## Parte 12: Dia a dia

| Quero... | Comando (no PowerShell da pasta) |
|---|---|
| Ver se está tudo ok | `powershell -ExecutionPolicy Bypass -File .\windows\ssh.ps1 mc status` |
| Ver se tem lag | `powershell -ExecutionPolicy Bypass -File .\windows\ssh.ps1 mc tps` |
| Entrar na VM | `powershell -ExecutionPolicy Bypass -File .\windows\ssh.ps1` |
| Baixar backup (**1x por semana**) | `powershell -ExecutionPolicy Bypass -File .\windows\pull-backup.ps1` |
| Aplicar mudanças do `config.env` | `powershell -ExecutionPolicy Bypass -File .\windows\deploy.ps1` |

Dentro da VM:
- `mc status`: situação geral
- `mc tps`: desempenho (abaixo de 50 ms = sem lag)
- `mc logs`: log ao vivo
- `mc restart`: reinicia o servidor
- `mc backup`: faz um backup agora
- `mc update`: atualiza
- `exit`: sai da VM

Para não precisar digitar `powershell -ExecutionPolicy Bypass -File` toda vez, rode uma vez `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned`. Depois disso, basta `.\windows\ssh.ps1 mc status`.

---

## Parte 13: Antes do fim dos 90 dias

O trial acaba em 90 dias **ou** quando os US$ 300 acabarem, o que vier primeiro. Acompanhe o gasto em **☰ → Faturamento**.

Na última semana:
1. Rode `.\windows\ssh.ps1 mc backup` e depois `.\windows\pull-backup.ps1`.
2. O script tem que terminar com **"Backup salvo e conferido"**. Guarde uma cópia do arquivo de `backups-local\` em outro lugar (pendrive ou Google Drive).
3. **NÃO faça upgrade para conta paga.**

Para continuar em outra VM (por exemplo, na Oracle):
1. Crie a VM nova.
2. Mude para `PROVIDER="oracle"` no `config.env` e rode o `deploy.ps1` com o novo IP, usuário e chave.
3. Rode:

   ```powershell
   .\windows\push-backup.ps1 -File .\backups-local\world-AAAAMMDD-HHMMSS.tar.zst
   ```

Mais detalhes no README.

---

## Deu problema?

| Problema | Solução |
|---|---|
| **Permission denied (publickey)** | O usuário do `new-key.ps1` tem que ser o mesmo que aparece na chave colada no GCP. Confira se você clicou em **Salvar** e se o **OS Login** está desligado (Parte 6). |
| **Connection timed out** | A VM está ligada (bolinha verde)? Você usou o **IP externo**? |
| O IP externo mudou | Acontece se a VM for desligada e ligada. Rode o deploy com o novo `-Ip`. Para jogar, nada muda: o jogo usa o IP do Tailscale. |
| O link do Tailscale não apareceu | Entre na VM com `ssh.ps1` e rode `sudo tailscale up`. |
| O amigo não conecta | Confira se o Tailscale dele está **Connected**, se ele aceitou o convite e se usou o IP `100.x`. |
| **Outdated client/server** | A versão no TLauncher tem que ser **1.21.11**. |
| Lag | Rode `mc tps`. Acima de 50 ms por tick, diminua `view-distance` em `/opt/minecraft/server/server.properties` e rode `mc restart`. |
| O servidor não sobe | Rode `mc logs` dentro da VM para ver o erro. |
