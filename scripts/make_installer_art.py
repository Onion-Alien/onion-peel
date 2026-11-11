"""Draw the picture on the right of the installer's pages (build-release.ps1 runs this; the
outputs are build files in target/installer, not committed):

- snip-{dark,light}-{100..200}.png  a small screen with the snip screen open on it: two
                                    windows, dimmed, a bright selection with its size, and
                                    the bar on top. One per display scale, so Setup never
                                    stretches it; light or dark windows to match Setup.

The bar, the dimming, the frame and the size label copy src/overlay.rs (its colours,
radii and Segoe UI), shrunk to fit. Nothing here is a real screenshot.

    python scripts/make_installer_art.py [--site]
Needs Pillow (pip install pillow).
"""
from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / "target" / "installer"
W, H = 270, 210  # the layout below is in these units
OUT_W = 220  # the picture's width on the pages at 100 % scale, in pixels (as Onion Launcher's)
OUT_H = round(H * OUT_W / W)
SCALES = (100, 125, 150, 175, 200)
SS = 4  # drawn this many times bigger, then scaled down, for smooth edges
UI = 0.72  # the snip screen's bar and label, as a share of their real size
FONT = Path("C:/Windows/Fonts/segoeui.ttf")

# src/overlay.rs
BAR_BG, BAR_EDGE, BTN_ON = (0x2B, 0x2B, 0x2B), (0x4A, 0x4A, 0x4A), (0x5C, 0x5C, 0x5C)
WHITE, DIM, BLACK = (255, 255, 255), (0xA0, 0xA0, 0xA0), (0, 0, 0)

THEMES = {
    # wallpaper top, bottom; window, title bar, lines, accent; picture edge
    "light": ((168, 205, 245), (96, 150, 225), (250, 250, 250), (236, 238, 242), (205, 210, 218), (16, 120, 214), (214, 214, 214)),
    "dark": ((64, 98, 156), (26, 44, 86), (58, 58, 62), (44, 44, 48), (96, 98, 106), (92, 200, 255), (64, 64, 64)),
}


def draw(theme: str, pct: int) -> Image.Image:
    top, bottom, win, title, line, accent, edge = THEMES[theme]
    k = pct / 100 * SS * OUT_W / W  # pixels per layout unit, in the big drawing
    px = lambda v: round(v * k)
    w, h = px(W), px(H)

    # the screen: a soft gradient wallpaper with two windows on it
    shade = Image.linear_gradient("L").resize((w, h))
    screen = Image.composite(Image.new("RGB", (w, h), bottom), Image.new("RGB", (w, h), top), shade)
    d = ImageDraw.Draw(screen)

    def window(x0, y0, x1, y1, rows):
        d.rounded_rectangle((px(x0), px(y0), px(x1), px(y1)), radius=px(6), fill=win)
        d.rounded_rectangle((px(x0), px(y0), px(x1), px(y0 + 16)), radius=px(6), fill=title)
        d.rectangle((px(x0), px(y0 + 10), px(x1), px(y0 + 16)), fill=title)
        for i, (a, b, colour) in enumerate(rows):
            y = y0 + 27 + i * 12
            d.rounded_rectangle((px(x0 + a), px(y), px(x0 + b), px(y + 5)), radius=px(2.5), fill=colour)

    window(14, 52, 168, 160, [(12, 100, line), (12, 124, line), (12, 76, line), (12, 56, accent), (12, 108, line), (12, 88, line)])
    window(98, 86, 256, 198, [(14, 84, line)])
    # in the front window, a little chart: what's being snipped
    cx0, cy0, cx1, cy1 = 112, 124, 242, 166
    d.rounded_rectangle((px(cx0), px(cy0), px(cx1), px(cy1)), radius=px(4), outline=line, width=max(1, px(1)))
    for i, hgt in enumerate((12, 21, 17, 27, 23, 31)):
        x = cx0 + 12 + i * 19
        d.rounded_rectangle((px(x), px(cy1 - 6 - hgt), px(x + 11), px(cy1 - 6)), radius=px(2), fill=accent)

    # the snip screen: everything dimmed to half, the selection bright with a white frame
    sel = (px(cx0 - 4), px(cy0 - 4), px(cx1 + 4), px(cy1 + 4))
    out = screen.point(lambda v: v >> 1)
    out.paste(screen.crop(sel), sel[:2])
    d = ImageDraw.Draw(out)
    f = max(SS, round(k * 1.5))  # the frame, about as thick as on screen
    d.rectangle((sel[0] - f - SS, sel[1] - f - SS, sel[2] + f + SS - 1, sel[3] + f + SS - 1), outline=BLACK, width=SS)
    d.rectangle((sel[0] - f, sel[1] - f, sel[2] + f - 1, sel[3] + f - 1), outline=WHITE, width=f)

    font = ImageFont.truetype(str(FONT), px(14 * UI))

    def text(box, s, colour):
        d.text(((box[0] + box[2]) / 2, (box[1] + box[3]) / 2), s, font=font, fill=colour, anchor="mm")

    # the size label under the selection, as on screen
    size = f"{round((sel[2] - sel[0]) / k * 3)} \u00d7 {round((sel[3] - sel[1]) / k * 3)}"
    lw = font.getlength(size) + px(16 * UI)
    gap = px(8 * UI) + f
    lbl = (sel[0], sel[3] + gap, sel[0] + lw, sel[3] + gap + px(24 * UI))
    d.rounded_rectangle(lbl, radius=px(6 * UI), fill=BAR_BG, outline=BAR_EDGE, width=max(1, SS // 2))
    text(lbl, size, WHITE)

    # the bar, centred at the top: Rectangle (on), Window, Full screen, close
    labels = ["Rectangle", "Window", "Full screen"]
    pad, gap, bh = px(16 * UI), px(4 * UI), px(37 * UI)
    widths = [font.getlength(s) + 2 * pad for s in labels] + [bh - px(8 * UI)]
    bw = sum(widths) + gap * (len(widths) - 1) + px(8 * UI)
    bx, by = (w - bw) / 2, px(10)
    d.rounded_rectangle((bx, by, bx + bw, by + bh), radius=px(8 * UI), fill=BAR_BG, outline=BAR_EDGE, width=max(1, SS // 2))
    x = bx + px(4 * UI)
    for i, bwid in enumerate(widths):
        b = (x, by + px(4 * UI), x + bwid, by + bh - px(4 * UI))
        if i == 0:
            d.rounded_rectangle(b, radius=px(4 * UI), fill=BTN_ON)
        if i < 3:
            text(b, labels[i], WHITE if i == 0 else DIM)
        else:
            cx, cy, s = (b[0] + b[2]) / 2, (b[1] + b[3]) / 2, px(5 * UI)
            for a, c in (((cx - s, cy - s), (cx + s, cy + s)), ((cx + s, cy - s), (cx - s, cy + s))):
                d.line((a, c), fill=DIM, width=max(1, px(1.5 * UI)))
        x += bwid + gap

    # rounded corners and a thin edge, like a picture card on the page
    mask = Image.new("L", (w, h))
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, w - 1, h - 1), radius=px(8), fill=255)
    card = Image.new("RGBA", (w, h))
    card.paste(out, mask=mask)
    ImageDraw.Draw(card).rounded_rectangle((0, 0, w - 1, h - 1), radius=px(8), outline=edge, width=max(1, round(k)))
    return card.resize((OUT_W * pct // 100, OUT_H * pct // 100), Image.LANCZOS)


def main() -> int:
    if "--site" in sys.argv[1:]:
        pic = ROOT / "docs" / "snip-dark.png"
        draw("dark", 420).save(pic, optimize=True)
        print(f"wrote {pic.relative_to(ROOT)}")
        return 0
    OUT.mkdir(parents=True, exist_ok=True)
    for theme in THEMES:
        for pct in SCALES:
            draw(theme, pct).save(OUT / f"snip-{theme}-{pct}.png")
    print(f"wrote the pictures to {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
