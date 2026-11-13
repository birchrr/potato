"""Draw assets/icon.ico, assets/icon.png and assets/logo.svg from assets/logo.txt. Needs Pillow.
Run after changing the logo: python scripts/icons.py"""
import os
from PIL import Image

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
lines = [l for l in open(os.path.join(ROOT, 'assets', 'logo.txt')).read().splitlines() if l and not l.startswith('#')]
COLORS = {l[0]: l[2:9] for l in lines if l[1:2] == '='}
rows = [l for l in lines if l[1:2] != '=']
w, h = len(rows[0]), len(rows)

# one pixel per cell on a 16x16 canvas, centred; every bigger size is this scaled up whole-pixel
base = Image.new('RGBA', (16, 16), (0, 0, 0, 0))
top = (16 - h) // 2
for y, r in enumerate(rows):
    for x, k in enumerate(r):
        if k in COLORS:
            base.putpixel((x, top + y), tuple(int(COLORS[k][i:i + 2], 16) for i in (1, 3, 5)) + (255,))
big = base.resize((256, 256), Image.NEAREST)
big.save(os.path.join(ROOT, 'assets', 'icon.png'))
big.save(os.path.join(ROOT, 'assets', 'icon.ico'), sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)])

rects = ''.join(f'<rect x="{x}" y="{y}" width="1" height="1" fill="{COLORS[k]}"/>' for y, r in enumerate(rows) for x, k in enumerate(r) if k in COLORS)
open(os.path.join(ROOT, 'assets', 'logo.svg'), 'w').write(
    f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w * 8}" height="{h * 8}" shape-rendering="crispEdges">{rects}</svg>\n')
