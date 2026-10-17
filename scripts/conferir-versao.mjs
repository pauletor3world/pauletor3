// Confere que a versão do app é a mesma em tauri.conf.json, app/package.json e Cargo.toml
// (workspace). Com uma tag (ex.: v2.1.0), confere também que ela bate. Diverge = falha.
//
//   node scripts/conferir-versao.mjs            # só os três arquivos
//   node scripts/conferir-versao.mjs v2.1.0     # e a tag

import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const read = (p) => readFileSync(join(root, p), "utf8");

const tauri = JSON.parse(read("app/src-tauri/tauri.conf.json")).version;
const pkg = JSON.parse(read("app/package.json")).version;
const ws = /\[workspace\.package\][^[]*?^version\s*=\s*"([^"]+)"/ms.exec(read("Cargo.toml"))?.[1];

const found = { "app/src-tauri/tauri.conf.json": tauri, "app/package.json": pkg, "Cargo.toml [workspace.package]": ws };
const tag = process.argv[2];
if (tag && tag.startsWith("v")) found[`tag ${tag}`] = tag.slice(1);

const values = new Set(Object.values(found));
if (values.size !== 1 || values.has(undefined)) {
  console.error("ERRO: a versão do app não é a mesma em todo lugar:");
  for (const [k, v] of Object.entries(found)) console.error(`  ${k}: ${v ?? "(não encontrada)"}`);
  console.error("Use scripts/publicar-app.ps1 -Versao X.Y.Z para trocar nos três de uma vez.");
  process.exit(1);
}
console.log(`versão ${tauri} confere em ${Object.keys(found).length} lugares`);
