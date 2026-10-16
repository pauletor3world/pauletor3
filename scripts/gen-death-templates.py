"""Gera core/src/death_templates.rs a partir do en_us.json do server.jar oficial.

Uso: python -I scripts/gen-death-templates.py caminho/para/en_us.json
(o en_us.json fica em META-INF/versions/<ver>/server-<ver>.jar -> assets/minecraft/lang/en_us.json)
"""
import json, sys

src = sys.argv[1]
out = sys.argv[2] if len(sys.argv) > 2 else "core/src/death_templates.rs"
d = json.load(open(src, encoding="utf-8"))
items = sorted((k, v) for k, v in d.items() if k.startswith("death.") and v.startswith("%1$s "))
adv = {k: d[k] for k in (
    "advancements.story.enter_the_nether.title",
    "advancements.story.enter_the_end.title",
    "advancements.end.kill_dragon.title",
)}
with open(out, "w", encoding="utf-8", newline="\n") as f:
    f.write("// GERADO por scripts/gen-death-templates.py a partir do en_us.json do server.jar 1.21.11.\n")
    f.write("// Não edite à mão.\n\n")
    f.write("/// (chave, modelo) das mensagens de morte do jogo. `%1$s` é a vítima.\n")
    f.write("pub const DEATH_TEMPLATES: &[(&str, &str)] = &[\n")
    for k, v in items:
        f.write(f"    ({json.dumps(k)}, {json.dumps(v)}),\n")
    f.write("];\n\n")
    f.write(f"pub const ADV_ENTER_NETHER: &str = {json.dumps(adv['advancements.story.enter_the_nether.title'])};\n")
    f.write(f"pub const ADV_ENTER_END: &str = {json.dumps(adv['advancements.story.enter_the_end.title'])};\n")
    f.write(f"pub const ADV_KILL_DRAGON: &str = {json.dumps(adv['advancements.end.kill_dragon.title'])};\n")
print(f"{len(items)} modelos")
