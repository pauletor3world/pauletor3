// Notas de uma versão, para a release e para o aviso "Atualização disponível" no app.
// Vêm da seção "## [X.Y.Z]" do CHANGELOG.md; sem ela, da mensagem da tag (variável
// NOTAS_DA_TAG); sem nenhuma das duas, um texto padrão.
//
//   node scripts/notas-da-versao.mjs 2.1.0 > notas.md

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const version = (process.argv[2] ?? "").replace(/^v/, "").trim();
if (!version) {
  console.error("uso: node scripts/notas-da-versao.mjs X.Y.Z");
  process.exit(2);
}

export function section(changelog, v) {
  const lines = changelog.split(/\r?\n/);
  const head = new RegExp(`^##\\s*\\[?${v.replace(/\./g, "\\.")}\\]?(\\s|$)`);
  const start = lines.findIndex((l) => head.test(l));
  if (start < 0) return "";
  const end = lines.findIndex((l, i) => i > start && /^##\s/.test(l));
  return lines
    .slice(start + 1, end < 0 ? undefined : end)
    .join("\n")
    .trim();
}

let notas = "";
try {
  notas = section(readFileSync(join(root, "CHANGELOG.md"), "utf8"), version);
} catch {
  // sem CHANGELOG.md
}
if (!notas) notas = (process.env.NOTAS_DA_TAG ?? "").trim();
if (!notas) notas = "Melhorias e correções. Atualize para continuar jogando com o grupo.";
process.stdout.write(notas + "\n");
