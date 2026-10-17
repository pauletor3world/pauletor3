// Página que o amigo abre pelo link do convite (normalmente no celular, pelo WhatsApp).
// A chave do mundo vem no fragmento (#...), que o navegador nunca manda ao servidor: o script
// da página monta o código do convite no próprio navegador.

import { LOGO_PNG } from "./logo";

const esc = (s: string) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);

export function invitePage(info: { valid: boolean; name: string; owner_nick: string }, downloadUrl: string): string {
  const name = esc(info.name || "pAuletOr 3");
  const owner = esc(info.owner_nick || "seu amigo");
  const dl = esc(downloadUrl || "#");
  return `<!doctype html>
<html lang="pt-BR">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="robots" content="noindex">
<title>Convite para ${name}</title>
<meta property="og:title" content="${owner} te chamou para o mundo ${name}">
<meta property="og:description" content="Baixe o app, abra o convite e jogue. Sem conta, sem configurar nada.">
<style>
:root{--bg:#0a1226;--card:#0e1830;--line:#1f2f55;--text:#eef2fa;--muted:#a3afcb;--lantern:#ffd278;--lantern-ink:#3a2410;--peach:#f0a474;--grass:#8acb5c}
*{box-sizing:border-box}
body{margin:0;background:radial-gradient(900px 420px at 50% 115%,rgba(240,164,116,.16),transparent 70%),linear-gradient(180deg,#0f1d40 0%,var(--bg) 45%);color:var(--text);font:16px/1.5 "Segoe UI Variable","Segoe UI",system-ui,-apple-system,Roboto,sans-serif;min-height:100vh}
main{max-width:520px;margin:0 auto;padding:28px 16px 40px}
.brand{display:flex;gap:10px;align-items:center;font-weight:700;font-size:17px}
.brand img{width:40px;height:40px}
h1{font-size:28px;line-height:1.2;margin:26px 0 8px;font-weight:700}
h1 b{color:var(--lantern)}
p.lead{color:var(--muted);margin:0 0 22px}
.card{background:var(--card);border:1px solid var(--line);border-radius:16px;padding:18px;margin:14px 0}
ol{list-style:none;padding:0;margin:0;counter-reset:s}
ol li{counter-increment:s;display:flex;gap:12px;align-items:center;padding:10px 0;border-bottom:1px solid var(--line)}
ol li:last-child{border:0}
ol li::before{content:counter(s);flex:0 0 28px;height:28px;border-radius:7px;background:rgba(255,210,120,.14);color:var(--lantern);display:grid;place-items:center;font-weight:700}
.btn{display:flex;justify-content:center;align-items:center;gap:8px;width:100%;padding:14px 16px;border-radius:12px;border:1px solid var(--line);background:#16254a;color:var(--text);font-weight:600;font-size:16px;text-decoration:none;cursor:pointer;margin-top:10px;font-family:inherit}
.btn.primary{background:var(--lantern);color:var(--lantern-ink);border:0;box-shadow:inset 0 2px 0 rgba(255,255,255,.4),inset 0 -3px 0 rgba(0,0,0,.2),0 10px 28px -12px rgba(255,210,120,.6)}
.btn:active{transform:translateY(2px)}
.btn:focus-visible{outline:2px solid var(--lantern);outline-offset:2px}
.note{font-size:14px;color:var(--muted);margin:0}
.warn{border-color:#4a3524;background:#1c1a24}
.warn b{color:var(--peach)}
code{background:#08101f;border:1px solid var(--line);border-radius:6px;padding:2px 6px;font-size:13px;word-break:break-all}
footer{margin-top:28px;font-size:12px;color:#6c7899;text-align:center}
#copied{display:none;color:var(--grass);text-align:center;margin-top:8px;font-size:14px}
</style>
</head>
<body>
<main>
  <div class="brand"><img src="${LOGO_PNG}" alt="" width="40" height="40">Machen Craft</div>
  ${
    info.valid
      ? `<h1>${owner} te chamou para o mundo <b>${name}</b></h1>
  <p class="lead">Um mundo de survival que fica salvo no PC de vocês. Sem servidor pago, sem conta, sem configurar nada.</p>
  <div class="card" id="pc-only" hidden>
    <p class="note">Você abriu no celular. Abra este mesmo link <b>no seu PC com Windows</b> (mande para você mesmo no WhatsApp Web, por exemplo).</p>
  </div>
  <div class="card">
    <ol>
      <li><span>Baixe e instale o <b>Machen Craft</b> no seu PC com Windows.</span></li>
      <li><span>Volte aqui e toque em <b>Abrir convite no app</b> (ou copie o convite e cole no app).</span></li>
      <li><span>Escolha seu nick e clique em <b>Jogar</b>.</span></li>
    </ol>
    <a class="btn primary" href="${dl}" id="dl">Baixar para Windows</a>
    <a class="btn" href="#" id="open">Abrir convite no app</a>
    <button class="btn" id="copy" type="button">Copiar convite</button>
    <div id="copied">Convite copiado. Abra o app e cole.</div>
  </div>
  <div class="card warn">
    <p class="note"><b>O Windows pode avisar que o app é desconhecido.</b> É normal enquanto o app não tem certificado da Microsoft. Clique em <b>Mais informações → Executar assim mesmo</b>.</p>
  </div>`
      : `<h1>Este convite não vale mais</h1>
  <p class="lead">Ele expirou ou já foi usado. Peça um convite novo para <b>${owner}</b>: leva um clique no app.</p>`
  }
  <footer>Machen Craft · não afiliado à Mojang nem à Microsoft.</footer>
</main>
<script>
(function(){
  var key = location.hash.slice(1);
  var parts = location.pathname.split("/c/");
  var token = parts.length > 1 ? parts[1] : "";
  if (!/Windows/i.test(navigator.userAgent) && /Android|iPhone|iPad/i.test(navigator.userAgent)) {
    var el = document.getElementById("pc-only"); if (el) el.hidden = false;
  }
  if (!key || !token) return;
  var payload = JSON.stringify({u: location.origin, t: token, k: key});
  var b64 = btoa(payload).replace(/\\+/g,"-").replace(/\\//g,"_").replace(/=+$/,"");
  var code = "P3J1." + b64;
  var open = document.getElementById("open");
  if (open) open.href = "machencraft://entrar/" + code;
  var copy = document.getElementById("copy");
  if (copy) copy.onclick = function(){
    var done = function(){ document.getElementById("copied").style.display = "block"; };
    if (navigator.clipboard) navigator.clipboard.writeText(code).then(done, function(){ prompt("Copie o convite:", code); });
    else prompt("Copie o convite:", code);
  };
})();
</script>
</body>
</html>`;
}
