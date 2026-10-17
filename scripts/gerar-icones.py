"""Gera os ícones do Machen Craft a partir da identidade (pasta identidade/).

Entradas (arte final, não editar aqui):
  identidade/machen-craft-v3-icone-2160.png        ícone do app (M de montanha)
  identidade/machen-craft-pixelart-v3-1920x1080.png cena em pixel art (início do app)

Saídas:
  app/src-tauri/icons/app-icon.png   (1024x1024, fonte para `npx tauri icon`)
  app/src-tauri/icons/tray-*.png     (32x32: idle, hosting, busy, problem)
  app/public/logo.png                (256x256, usado na interface)
  app/public/cena.png                (cena do início, sem perdas)

Uso: python -I scripts/gerar-icones.py && cd app && npx tauri icon src-tauri/icons/app-icon.png
"""
import os
import shutil

from PIL import Image, ImageDraw

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ID = os.path.join(ROOT, "identidade")
ICONS = os.path.join(ROOT, "app", "src-tauri", "icons")
PUBLIC = os.path.join(ROOT, "app", "public")

icon = Image.open(os.path.join(ID, "machen-craft-v3-icone-2160.png")).convert("RGBA")
icon.resize((1024, 1024), Image.LANCZOS).save(os.path.join(ICONS, "app-icon.png"))
icon.resize((256, 256), Image.LANCZOS).save(os.path.join(PUBLIC, "logo.png"))
shutil.copyfile(os.path.join(ID, "machen-craft-pixelart-v3-1920x1080.png"), os.path.join(PUBLIC, "cena.png"))


def tray(state):
    # Na bandeja (32 px) a moldura do ícone come espaço: usa só o miolo, mais justo no M.
    S = 256
    w = icon.width
    box = (int(w * 0.09), int(w * 0.13), int(w * 0.91), int(w * 0.95))
    img = icon.crop(box).resize((S, S), Image.LANCZOS)
    mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, S - 1, S - 1], radius=S * 0.22, fill=255)
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(img, (0, 0), mask)
    colors = {"hosting": (124, 196, 82), "busy": (127, 176, 230), "problem": (226, 114, 95)}
    if state in colors:
        d = ImageDraw.Draw(out)
        d.ellipse([S * 0.56, S * 0.56, S, S], fill=(11, 21, 43, 255))
        d.ellipse([S * 0.63, S * 0.63, S * 0.93, S * 0.93], fill=colors[state] + (255,))
    return out.resize((32, 32), Image.LANCZOS)


for s in ("idle", "hosting", "busy", "problem"):
    tray(s).save(os.path.join(ICONS, f"tray-{s}.png"))
print("ok")
