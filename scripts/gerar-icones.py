"""Gera o ícone do app pAuletOr 3 (arte própria, sem assets da Mojang).

Três camadas de colinas (o "3" do nome) sob um céu noturno com um sol âmbar.
Saídas:
  app/src-tauri/icons/app-icon.png   (1024x1024, fonte para `tauri icon`)
  app/src-tauri/icons/tray-*.png     (32x32: idle, hosting, busy, problem)
  app/public/logo.png                (256x256, usado na interface)

Uso: python -I scripts/gerar-icones.py
"""
import math
import os

from PIL import Image, ImageDraw, ImageFilter

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICONS = os.path.join(ROOT, "app", "src-tauri", "icons")
PUBLIC = os.path.join(ROOT, "app", "public")
os.makedirs(ICONS, exist_ok=True)
os.makedirs(PUBLIC, exist_ok=True)


def lerp(a, b, t):
    return tuple(int(a[i] + (b[i] - a[i]) * t) for i in range(len(a)))


def emblem(size, ss=3, rounded=True):
    S = size * ss
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))

    # Fundo: céu noturno em gradiente vertical, cantos arredondados.
    sky = Image.new("RGBA", (S, S))
    top, bottom = (16, 24, 44, 255), (22, 58, 60, 255)
    px = sky.load()
    for y in range(S):
        c = lerp(top, bottom, y / (S - 1))
        for x in range(S):
            px[x, y] = c
    mask = Image.new("L", (S, S), 0)
    md = ImageDraw.Draw(mask)
    r = int(S * 0.22) if rounded else 0
    md.rounded_rectangle([0, 0, S - 1, S - 1], radius=r, fill=255)
    img.paste(sky, (0, 0), mask)

    d = ImageDraw.Draw(img)
    # Estrelas.
    for (fx, fy, fr) in [(0.18, 0.18, 0.010), (0.30, 0.10, 0.007), (0.42, 0.24, 0.006), (0.12, 0.34, 0.006), (0.55, 0.14, 0.008)]:
        cx, cy, rr = fx * S, fy * S, fr * S
        d.ellipse([cx - rr, cy - rr, cx + rr, cy + rr], fill=(230, 236, 255, 200))

    # Sol âmbar com halo.
    sun_c, sun_r = (0.72 * S, 0.30 * S), 0.12 * S
    halo = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    hd = ImageDraw.Draw(halo)
    hd.ellipse([sun_c[0] - sun_r * 1.9, sun_c[1] - sun_r * 1.9, sun_c[0] + sun_r * 1.9, sun_c[1] + sun_r * 1.9], fill=(255, 190, 90, 90))
    halo = halo.filter(ImageFilter.GaussianBlur(S * 0.04))
    img = Image.alpha_composite(img, halo)
    d = ImageDraw.Draw(img)
    d.ellipse([sun_c[0] - sun_r, sun_c[1] - sun_r, sun_c[0] + sun_r, sun_c[1] + sun_r], fill=(255, 196, 92, 255))

    # Três colinas (de trás para frente).
    layers = [
        (0.56, 0.20, 1.3, 0.0, (44, 120, 104, 255)),
        (0.66, 0.16, 1.7, 1.4, (52, 160, 112, 255)),
        (0.77, 0.12, 1.1, 2.6, (98, 204, 128, 255)),
    ]
    for base, amp, freq, phase, color in layers:
        pts = [(0, S)]
        for i in range(0, 101):
            x = i / 100
            y = base - amp * (0.55 * math.sin(math.pi * x * freq + phase) ** 2 + 0.45 * math.sin(math.pi * x)) * 0.9
            pts.append((x * S, y * S))
        pts.append((S, S))
        layer = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        ImageDraw.Draw(layer).polygon(pts, fill=color)
        img = Image.alpha_composite(img, layer)

    # Recorta tudo pela máscara arredondada.
    out = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    out.paste(img, (0, 0), mask)
    return out.resize((size, size), Image.LANCZOS)


def tray(state):
    base = emblem(32, ss=8, rounded=True)
    colors = {"hosting": (70, 220, 120), "busy": (255, 190, 70), "problem": (240, 80, 80)}
    if state in colors:
        d = ImageDraw.Draw(base)
        d.ellipse([19, 19, 31, 31], fill=(12, 16, 28, 255))
        d.ellipse([21, 21, 29, 29], fill=colors[state] + (255,))
    return base


emblem(1024).save(os.path.join(ICONS, "app-icon.png"))
emblem(256).save(os.path.join(PUBLIC, "logo.png"))
for s in ["idle", "hosting", "busy", "problem"]:
    tray(s).save(os.path.join(ICONS, f"tray-{s}.png"))
print("ícones gerados")
