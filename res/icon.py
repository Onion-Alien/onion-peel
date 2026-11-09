"""Draws res/onion-peel.ico: a blue tile with a lighter selection and white crop corners.

Run `python res/icon.py` after changing it; the build embeds the .ico in onion-peel.exe.
"""
from pathlib import Path

from PIL import Image, ImageDraw

SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
TOP, BOTTOM = (92, 200, 255), (16, 120, 214)  # tile gradient, around the snip screen's accent
SS = 8  # drawn 8x larger, then shrunk: smooth rounded corners


def draw(n: int) -> Image.Image:
    big = n * SS
    # Lengths are in 16ths of the icon, snapped to whole final pixels so small sizes stay crisp.
    u = lambda v: round(v * n / 16) * SS

    shade = Image.linear_gradient("L").resize((big, big))
    tile = Image.composite(Image.new("RGBA", (big, big), BOTTOM), Image.new("RGBA", (big, big), TOP), shade)
    mask = Image.new("L", (big, big))
    ImageDraw.Draw(mask).rounded_rectangle((u(1), u(1), big - u(1) - 1, big - u(1) - 1), radius=u(3), fill=255)
    img = Image.new("RGBA", (big, big))
    img.paste(tile, mask=mask)

    # The selection: lighter inside, like the bright area on the snip screen.
    x0, y0 = u(4), u(5)
    sel = Image.new("RGBA", (big, big))
    ImageDraw.Draw(sel).rectangle((x0, y0, big - x0 - 1, big - y0 - 1), fill=(255, 255, 255, 72))
    img.alpha_composite(sel)

    # One crop corner just outside the selection's top-left, then mirrored to the other three.
    w = SS if n <= 20 else max(u(1.25), 2 * SS)  # line width: 1 pixel on the tiny sizes, then at least 2
    across, down = u(2.5), u(2)  # arm lengths, short enough to leave clear gaps between corners
    corner = Image.new("RGBA", (big, big))
    d = ImageDraw.Draw(corner)
    d.rectangle((x0 - w, y0 - w, x0 + across - 1, y0 - 1), fill="white")
    d.rectangle((x0 - w, y0 - w, x0 - 1, y0 + down - 1), fill="white")
    for flip in (None, Image.FLIP_LEFT_RIGHT, Image.FLIP_TOP_BOTTOM, Image.ROTATE_180):
        img.alpha_composite(corner if flip is None else corner.transpose(flip))
    return img.resize((n, n), Image.BOX)  # plain averaging keeps snapped edges sharp


if __name__ == "__main__":
    out = Path(__file__).with_name("onion-peel.ico")
    imgs = [draw(n) for n in SIZES]
    imgs[-1].save(out, sizes=[(n, n) for n in SIZES], append_images=imgs[:-1])
    print(out.name, out.stat().st_size, "bytes")
