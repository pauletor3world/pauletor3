# Servidor Minecraft 1.21.11 (Fabric + Lithium) no Google Cloud

Minecraft Java **1.21.11** com **Fabric Loader + Lithium** (otimização) e Java 21, numa VM do Google Cloud (free trial: US$ 300 por 90 dias). O mesmo código serve para a Oracle Cloud: troque `PROVIDER` no `config.env`.

- O **Lithium roda só no servidor**. Os jogadores entram com o **cliente vanilla 1.21.11**, sem mods. Não há outros mods ou plugins.
- O servidor está em **modo offline** (aceita TLauncher) e **sem whitelist**. O acesso é pela rede privada do **Tailscale**, e a porta pública fica fechada.

> **Começando do zero?** Siga o [PASSO-A-PASSO.md](PASSO-A-PASSO.md): da criação do projeto no Google Cloud até os amigos entrarem no jogo.

## ⚠️ Modo offline: decida antes do primeiro boot

O servidor vem configurado com `online-mode=false` e `enforce-secure-profile=false`.

- **Decida isso antes de ligar o servidor pela primeira vez.** No modo offline, o UUID de cada jogador é calculado a partir do nick. No modo online, ele vem da conta Microsoft. Se você trocar de modo depois, todo mundo entra como um jogador novo: perde inventário, posição e o status de op.
- O `setup.sh` só copia o `server.properties` se ele ainda não existir na VM. Para mudar o modo, edite `server/server.properties` **antes** do primeiro deploy.
- **Não há senha por nick.** Quem está na sua rede Tailscale pode entrar com qualquer nick. Só compartilhe a VM com quem você confia e dê op com cuidado.
- **Nunca** use `PUBLIC_PORT=1` neste modo. O `setup.sh` avisa se isso acontecer.

## Arquivos

| Arquivo | Para que serve |
|---|---|
| `config.env.example` | Variáveis editáveis: `PROVIDER`, RAM, MOTD, porta, `PUBLIC_PORT`, Tailscale. Copie para `config.env`. |
| `server/setup.sh` | Instalação completa na VM. É idempotente: rodar de novo só aplica o que mudou. |
| `server/start.sh` | Inicia o Fabric com G1GC (`-Xms`/`-Xmx` vêm do `config.env`, padrão 5G). |
| `server/server.properties` | Template, usado só no primeiro deploy. |
| `server/systemd/*` | Serviço do servidor e timer do backup diário (05:00, horário de Brasília). |
| `server/tools/update-server.sh` | Baixa o server.jar vanilla (confere o SHA-1 da Mojang), o launcher do Fabric e o Lithium (confere o SHA-512 do Modrinth). Faz troca atômica e guarda o anterior em `.bak`. |
| `server/tools/mc` | CLI de admin, instalado como `mc` na VM. |
| `server/tools/rcon.py` | Cliente RCON em Python puro. |
| `server/tools/backup.sh` | Backup dos mundos em `.tar.zst`. Mantém os 7 últimos. |
| `server/tools/stop.sh` | Parada limpa: manda `stop` via RCON e espera o servidor salvar. |
| `server/tools/worldinfo.py` | Lê a versão de um `level.dat`. Usado pelo `mc import-world`. |
| `windows/new-key.ps1` | Cria a chave SSH (`%USERPROFILE%\.ssh\gcp-mc`) e mostra a chave pública para colar no GCP. |
| `windows/deploy.ps1` | Envia tudo para a VM e roda o `setup.sh`. |
| `windows/ssh.ps1` | Abre um terminal na VM, ou roda um comando nela: `.\windows\ssh.ps1 mc status`. |
| `windows/push-world.ps1` | Envia um mundo do seu PC (pasta com `level.dat`) e o coloca no servidor. |
| `windows/pull-backup.ps1` | Baixa o backup mais recente para `.\backups-local\`. |
| `windows/push-backup.ps1` | Envia um backup baixado e restaura na VM. Serve para migrar de provedor. |

O IP, a chave e o usuário ficam salvos em `windows\local.json`, que o git ignora. Na VM, tudo fica em `/opt/minecraft`: `server/` (mundo, jars e `mods/`), `backups/`, `tools/`, `config.env` e `rcon.secret`.

## Usuário SSH no Google Cloud

No GCP **não existe o usuário `ubuntu`**. O usuário é o nome que está no fim da chave pública: o `new-key.ps1 -User machen` gera a chave com o comentário `machen`. O GCP cria esse usuário na VM, com `sudo`, quando você cola a chave em **VM → Editar → Chaves SSH**. Os scripts do Windows usam o usuário salvo em `windows\local.json`.

### OS Login

Se o **OS Login** estiver ligado, o GCP **ignora as chaves SSH coladas nos metadados** e o deploy dá `Permission denied (publickey)`. Para verificar:

1. Abra **Compute Engine → Configurações → Metadados** (Metadata) no projeto. Se houver a chave `enable-oslogin` com valor `TRUE`, o OS Login está ligado no projeto inteiro.
2. Na VM (**Editar → Metadados**), veja se existe `enable-oslogin`.
3. Ou, no Cloud Shell: `gcloud compute project-info describe --format="value(commonInstanceMetadata.items)"`.

Para desligar só nesta VM: em **VM → Editar → Metadados → Adicionar item**, coloque a chave `enable-oslogin` com valor `FALSE` e salve. O valor na VM tem prioridade sobre o do projeto. Contas pessoais do free trial normalmente vêm com o OS Login desligado.

## Firewall

| Provedor | O que o `setup.sh` faz |
|---|---|
| `gcp` | Ativa o `ufw`: nega tudo que entra, libera SSH (22) e libera a porta 25565/tcp **só na interface `tailscale0`**. |
| `oracle` | Insere regras no iptables antes do REJECT da imagem da Oracle e salva com `netfilter-persistent`. |

O RCON (25575) nunca é exposto.

**Não é preciso criar regra no firewall da VPC do GCP para o Tailscale.** Segundo a documentação do Tailscale ([Firewall ports](https://tailscale.com/kb/1082/firewall-ports)), na maioria dos casos nenhuma porta precisa ser aberta. As conexões partem de dentro para fora, e quando a conexão direta não é possível o tráfego passa pelos relays DERP. O firewall padrão da VPC já libera o SSH (regra `default-allow-ssh`). Só seria preciso mexer na VPC se você abrisse a porta pública.

## Primeiro deploy (resumo; detalhes no PASSO-A-PASSO)

```powershell
powershell -ExecutionPolicy Bypass -File .\windows\new-key.ps1 -User machen
# cole a chave pública na VM: Editar > Chaves SSH > Adicionar item > Salvar
powershell -ExecutionPolicy Bypass -File .\windows\deploy.ps1 -Ip IP_EXTERNO_DA_VM
```

No fim, abra o **link do Tailscale** que o setup mostrar. Depois veja o IP `100.x.y.z` com `.\windows\ssh.ps1 mc status` e conecte no jogo.

## Importar um mundo que você já tem

1. No PC, encontre a pasta do mundo (a que tem `level.dat`). Mundos de singleplayer do TLauncher e do launcher oficial ficam em `%APPDATA%\.minecraft\saves\NOME_DO_MUNDO`.
2. Feche o Minecraft e rode:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\windows\push-world.ps1 -World "$env:APPDATA\.minecraft\saves\NOME_DO_MUNDO"
   ```

3. O script compacta, envia e roda `mc import-world` na VM. O mundo que estava na VM ganha um backup e é guardado em `server/pre-import-*`.

Detalhes:
- **Versão:** um mundo de versão **mais nova** que a 1.21.11 é recusado, porque o jogo não consegue abrir. Um mundo de versão mais antiga é convertido automaticamente quando o servidor abre. Faça uma cópia antes, por garantia.
- **Mundos de servidor Paper/Spigot** (com `mundo_nether` e `mundo_the_end` separados): envie um `.zip` com as três pastas. O `mc import-world` junta tudo.
- **Inventários:** o servidor procura cada jogador em `world/playerdata/<UUID>.dat`. No modo offline, o UUID vem do nick (`mc uuid <nick>`). Se alguém entrar sem o inventário antigo, veja a lista de UUIDs que o `import-world` mostrou e rode `mc player-move <uuid-antigo> <nick>`. Os arquivos originais são mantidos.

## Dia a dia (na VM, via `.\windows\ssh.ps1`)

```
mc status              serviço, versões, jogadores, IP do Tailscale, último backup
mc tps                 desempenho: tempo médio de tick (< 50 ms = sem lag)
mc logs                log ao vivo (Ctrl+C sai)
mc cmd "say oi"        comando no console
mc op <nick>           dá operador (mc deop <nick> tira)
mc backup              backup agora
mc backups             lista backups
mc restore <arquivo>   restaura um backup (o mundo atual vai para server/pre-restore-*)
mc import-world <arq>  importa um mundo (.tar.gz/.tar.zst/.zip)
mc uuid <nick>         UUID offline de um nick
mc player-move <uuid> <nick>   passa inventário/posição de um UUID antigo para um nick
mc update              para, atualiza vanilla/Fabric/Lithium e sobe de novo
mc restart | start | stop
```

O `mc tps` usa o comando vanilla `/tick query`. Abaixo de 50 ms por tick, o servidor mantém 20 TPS sem lag.

## Backups

O backup diário roda sozinho às 05:00. **Uma vez por semana, rode `.\windows\pull-backup.ps1`** para ter uma cópia fora da nuvem.

## Antes do fim dos 90 dias

O free trial acaba em 90 dias **ou** quando os US$ 300 acabarem, o que vier primeiro. Uma e2-standard-2 em São Paulo custa em torno de US$ 0,10/h: cerca de US$ 230 em 90 dias, mais disco e IP. Cabe no crédito, mas com pouca folga. Acompanhe em **Faturamento → Relatórios**. Se quiser, crie um alerta em **Faturamento → Orçamentos e alertas**.

Na última semana:

1. Rode `.\windows\pull-backup.ps1`. Se quiser o estado mais recente possível, rode antes `.\windows\ssh.ps1 mc backup`.
2. **Confira o arquivo baixado.** O script testa o arquivo na VM (`zstd -t`) e compara o SHA-256 dos dois lados. Ele precisa terminar com **"Backup salvo e conferido"**. Confira também se o arquivo está em `.\backups-local\` com um tamanho coerente (dezenas ou centenas de MB), e guarde uma cópia em outro lugar (pendrive ou Google Drive).
3. **NÃO faça upgrade para conta paga.** Sem o upgrade, o Google não cobra nada. Ao fim do trial, as VMs são desligadas, e o que ficar no projeto é apagado depois de um tempo.

### Restaurar em outra VM (ex.: Oracle)

1. Crie a VM nova. Na Oracle, siga o caminho Oracle do PASSO-A-PASSO.
2. No `config.env`, troque para `PROVIDER="oracle"` (na Oracle, pode subir `MC_XMS`/`MC_XMX` para `6G`).
3. Rode o deploy na VM nova: `.\windows\deploy.ps1 -Ip NOVO_IP -User ubuntu -Key C:\caminho\da\chave.key`.
4. Envie e restaure o backup:

   ```powershell
   powershell -ExecutionPolicy Bypass -File .\windows\push-backup.ps1 -File .\backups-local\world-AAAAMMDD-HHMMSS.tar.zst
   ```

   Isso roda `mc restore` na VM. O mundo gerado no primeiro boot é guardado em `server/pre-restore-*`.

5. Na Oracle, faça login no Tailscale de novo pelo link. Os amigos precisam do novo IP `100.x` (e de um novo convite de compartilhamento).

O Fabric e o Lithium são Java puro: os mesmos arquivos rodam em x86 (GCP) e ARM (Oracle).

## Estabilidade

- O servidor sobe sozinho no boot e reinicia se cair (`Restart=on-failure`).
- `mc stop`, `restart` e `update` mandam `stop` via RCON e esperam o mundo ser salvo. O SIGTERM fica como reserva.
- Rodar o deploy de novo só reinicia o servidor se algo mudou (jars, config, `server.properties` ou o serviço).
- As atualizações de segurança do Ubuntu são aplicadas automaticamente (`unattended-upgrades`).
- O IP externo do GCP é **efêmero** e pode mudar se a VM for desligada e ligada. Para jogar, isso não importa (o jogo usa o IP do Tailscale). Para os scripts, rode o deploy com o novo `-Ip`, ou use o IP `100.x` do Tailscale como `-Ip`.
