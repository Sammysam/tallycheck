"""Mac App Store screenshots: puts each app screenshot in a macOS-style window (title bar with the
three window buttons, rounded corners, soft shadow) on an original desktop background, at exactly
2880 x 1800.

    python3 tools/appstore_frame.py <screenshots-dir> <output-dir> [--title TallyCheck]

Expects screenshots of the app's content at 2640 x 1560 (appstore-shots.mjs with
SHOT_SIZE=1320x780). Needs Pillow (pip install pillow).
"""
import argparse
import math
import os
import random

from PIL import Image, ImageDraw, ImageFilter, ImageFont

W, H = 2880, 1800
WIN_W, CONTENT_H, BAR = 2640, 1560, 56
RADIUS = 22


def background():
    """An original soft gradient: deep navy to teal, with blurred light blobs."""
    top, bottom = (16, 30, 66), (17, 94, 110)
    bg = Image.new('RGB', (W, H))
    px = ImageDraw.Draw(bg)
    for y in range(H):
        t = y / (H - 1)
        px.line([(0, y), (W, y)], fill=tuple(round(top[i] + (bottom[i] - top[i]) * t) for i in range(3)))
    glow = Image.new('RGBA', (W, H), (0, 0, 0, 0))
    g = ImageDraw.Draw(glow)
    random.seed(7)
    for cx, cy, r, col in [(420, 260, 700, (249, 115, 22, 70)), (2500, 380, 820, (56, 189, 248, 70)),
                           (1500, 1650, 900, (20, 184, 166, 80)), (2700, 1600, 600, (167, 139, 250, 55))]:
        g.ellipse([cx - r, cy - r, cx + r, cy + r], fill=col)
    glow = glow.filter(ImageFilter.GaussianBlur(260))
    bg = Image.alpha_composite(bg.convert('RGBA'), glow)
    # A few gentle diagonal light bands.
    bands = Image.new('RGBA', (W, H), (0, 0, 0, 0))
    b = ImageDraw.Draw(bands)
    for k in range(5):
        x0 = -800 + k * 900
        b.polygon([(x0, H), (x0 + 380, H), (x0 + 380 + H * 0.55, 0), (x0 + H * 0.55, 0)], fill=(255, 255, 255, 9))
    bands = bands.filter(ImageFilter.GaussianBlur(40))
    return Image.alpha_composite(bg, bands)


def font(size):
    for f in ['/System/Library/Fonts/SFNS.ttf', '/System/Library/Fonts/Helvetica.ttc',
              '/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf']:
        if os.path.exists(f):
            return ImageFont.truetype(f, size)
    return ImageFont.load_default()


def window(shot, title):
    win_h = BAR + CONTENT_H
    win = Image.new('RGBA', (WIN_W, win_h), (0, 0, 0, 0))
    bar = Image.new('RGBA', (WIN_W, BAR), (236, 236, 238, 255))
    d = ImageDraw.Draw(bar)
    for i, col in enumerate([(255, 95, 87), (254, 188, 46), (40, 200, 64)]):
        cx = 36 + i * 40
        d.ellipse([cx - 12, BAR // 2 - 12, cx + 12, BAR // 2 + 12], fill=col + (255,))
    f = font(26)
    tw = d.textlength(title, font=f)
    d.text(((WIN_W - tw) / 2, BAR / 2 - 15), title, font=f, fill=(60, 60, 67, 255))
    d.line([(0, BAR - 1), (WIN_W, BAR - 1)], fill=(210, 210, 214, 255), width=2)
    win.paste(bar, (0, 0))
    win.paste(shot.convert('RGBA').resize((WIN_W, CONTENT_H), Image.LANCZOS), (0, BAR))
    mask = Image.new('L', win.size, 0)
    ImageDraw.Draw(mask).rounded_rectangle([0, 0, WIN_W - 1, win_h - 1], RADIUS, fill=255)
    win.putalpha(mask)
    return win


def frame(bg, shot, title):
    win = window(shot, title)
    x, y = (W - win.width) // 2, (H - win.height) // 2
    shadow = Image.new('RGBA', (W, H), (0, 0, 0, 0))
    ImageDraw.Draw(shadow).rounded_rectangle([x, y + 30, x + win.width, y + win.height + 30], RADIUS, fill=(0, 0, 0, 150))
    shadow = shadow.filter(ImageFilter.GaussianBlur(45))
    out = Image.alpha_composite(bg, shadow)
    out.alpha_composite(win, (x, y))
    return out.convert('RGB')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('src')
    p.add_argument('out')
    p.add_argument('--title', default='TallyCheck')
    p.add_argument('--background-only', action='store_true')
    a = p.parse_args()
    os.makedirs(a.out, exist_ok=True)
    bg = background()
    if a.background_only:
        bg.convert('RGB').save(os.path.join(a.out, 'desktop-background.png'))
        return
    for name in sorted(os.listdir(a.src)):
        if not name.lower().endswith('.png') or name.startswith('FAILED'):
            continue
        frame(bg, Image.open(os.path.join(a.src, name)), a.title).save(os.path.join(a.out, name), optimize=True)
        print('framed', name)


if __name__ == '__main__':
    main()
