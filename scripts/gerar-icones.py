"""Gera os ícones do Machenia a partir da identidade (pasta identidade/).

Entradas (arte final, não editar aqui):
  identidade/machen-craft-noite-limpo-1024.png     ícone do app (mundo fechado)
  identidade/machen-craft-dia-fundo-noite-limpo-1024.png  logo com o mundo aberto (interface e bandeja)
  identidade/machen-craft-pixelart-v3-1920x1080.png cena em pixel art (início do app)

Saídas:
  app/src-tauri/icons/app-icon.png   (1024x1024, fonte para `npx tauri icon`)
  app/src-tauri/icons/tray-*.png     (32x32: idle, hosting, busy, problem)
  app/public/logo.png                (256x256, usado na interface)
  app/public/logo-dia.png            (256x256, logo com o mundo aberto)
  app/public/cena.png                (cena do assistente, sem perdas)
  coordinator/src/logo.ts            (96x96, página do convite)
  docs/img/logo.png                  (256x256, README)

Uso: python -I scripts/gerar-icones.py && cd app && npx tauri icon src-tauri/icons/app-icon.png
"""
import base64
import io
import os
import shutil

from PIL import Image, ImageDraw, ImageFont

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ID = os.path.join(ROOT, "identidade")
ICONS = os.path.join(ROOT, "app", "src-tauri", "icons")
PUBLIC = os.path.join(ROOT, "app", "public")

icon = Image.open(os.path.join(ID, "machen-craft-noite-limpo-1024.png")).convert("RGBA")
day = Image.open(os.path.join(ID, "machen-craft-dia-fundo-noite-limpo-1024.png")).convert("RGBA")
icon.resize((1024, 1024), Image.LANCZOS).save(os.path.join(ICONS, "app-icon.png"))
icon.resize((256, 256), Image.LANCZOS).save(os.path.join(PUBLIC, "logo.png"))
day.resize((256, 256), Image.LANCZOS).save(os.path.join(PUBLIC, "logo-dia.png"))
shutil.copyfile(os.path.join(ID, "machen-craft-pixelart-v3-1920x1080.png"), os.path.join(PUBLIC, "cena.png"))


def tray(state):
    # Na bandeja (32 px) a moldura do ícone come espaço: usa só o miolo, mais justo nas montanhas.
    S = 256
    w = icon.width
    box = (int(w * 0.11), int(w * 0.12), int(w * 0.89), int(w * 0.90))
    # hospedando: a logo de dia (mundo aberto); o resto, a de noite
    img = (day if state == "hosting" else icon).crop(box).resize((S, S), Image.LANCZOS)
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

# página do convite (coordenação) e README
buf = io.BytesIO()
icon.resize((96, 96), Image.LANCZOS).save(buf, "PNG", optimize=True)
with open(os.path.join(ROOT, "coordinator", "src", "logo.ts"), "w", encoding="utf-8", newline="\n") as f:
    f.write("// Ícone do Machenia (96 px), embutido na página do convite. Gerado de identidade/.\n")
    f.write(f'export const LOGO_PNG = "data:image/png;base64,{base64.b64encode(buf.getvalue()).decode()}";\n')
shutil.copyfile(os.path.join(PUBLIC, "logo.png"), os.path.join(ROOT, "docs", "img", "logo.png"))


# Instalador (NSIS): imagem lateral (boas-vindas e fim) e cabeçalho (telas do meio).
# Saem da cena noturna, com a logo e o nome. Fonte: Segoe UI do Windows.
def font(size):
    for f in ("segoeuib.ttf", "C:/Windows/Fonts/segoeuib.ttf", "DejaVuSans-Bold.ttf"):
        try:
            return ImageFont.truetype(f, size)
        except OSError:
            pass
    return ImageFont.load_default()


night = Image.open(os.path.join(ID, "machen-craft-noite-fumaca-1280x720.gif")).convert("RGB")


def scene(w, h, cx, cy, scale):
    """Recorte da cena (centro cx, cy em px da arte) ampliado em pixel art."""
    sw, sh = round(w / scale), round(h / scale)
    x0 = max(0, min(night.width - sw, cx - sw // 2))
    y0 = max(0, min(night.height - sh, cy - sh // 2))
    return night.crop((x0, y0, x0 + sw, y0 + sh)).resize((w, h), Image.NEAREST)


def shade(img, top, bottom):
    """Escurece de cima para baixo (alfa de 0 a 255) com o azul da noite."""
    w, h = img.size
    grad = Image.new("L", (1, h))
    for y in range(h):
        t = y / (h - 1)
        grad.putpixel((0, y), round(top + (bottom - top) * t))
    over = Image.new("RGB", (w, h), (8, 13, 30))
    return Image.composite(over, img, grad.resize((w, h)))


# lateral 164x314: a casa embaixo, céu com logo e nome em cima
W, H = 164, 314
side = scene(W, H, 655, 330, 0.75)
side = shade(side, 150, 0)
logo = icon.resize((64, 64), Image.LANCZOS)
side.paste(logo, ((W - 64) // 2, 26), logo)
d = ImageDraw.Draw(side)
for text, y, f, color in (("Machenia", 102, font(23), (238, 242, 250)),):
    tw = d.textlength(text, font=f)
    d.text(((W - tw) / 2, y), text, font=f, fill=color)
side.save(os.path.join(ICONS, "instalador-lateral.bmp"))

# cabeçalho 150x57: faixa da cena com a casa, logo à direita
head = scene(150, 57, 640, 470, 0.3)
head = shade(head, 40, 0)
small = icon.resize((41, 41), Image.LANCZOS)
head.paste(small, (150 - 41 - 8, 8), small)
head.save(os.path.join(ICONS, "instalador-cabecalho.bmp"))
print("ok")
