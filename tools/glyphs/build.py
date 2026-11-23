#!/usr/bin/env python3
"""Build the chrome's two glyph fonts from their SVG sources.

    python tools/glyphs/build.py [icons|avatars] [--preview DIR]

needs `fonttools` and `skia-pathops` (and `pillow` for `--preview`):

    python -m venv .venv && .venv/bin/pip install fonttools skia-pathops pillow

It reads

    crates/osmena-chrome/assets/icons/<name>.svg     the chrome's own glyphs
    crates/osmena-chrome/assets/avatars/<name>.svg   the profile avatars

and writes `OsmenaIcons.ttf` and `OsmenaAvatars.ttf` beside the other
fonts. The icons are the repository's own work under its own licence;
the avatars are too, except the whale, which is Noto Emoji's and makes
that font one under the SIL Open Font License
(`LICENSE-OsmenaAvatars.txt`). Which glyph sits at which code point is read from the Rust
sources, which stay the one place that says so: `Icon::code` in
`osmena-chrome/src/icons.rs` and `Avatar::glyph` in
`osmena-core/src/profile.rs`. A variant's file is its name in
kebab-case (`Icon::StarFilled` is `star-filled.svg`). A variant with no
file, or a file with no variant, stops the build.

The drawings are plain SVG, small enough to write by hand, on a
16-unit grid (`viewBox="0 0 16 16"`): one unit is one pixel where the
chrome draws the glyph at its natural size, so a line drawn on the
grid is a sharp line on the screen. What the builder understands:

  * `path`, `circle`, `ellipse`, `rect` (with `rx`), `line`, `polyline`,
    `polygon`, and `g`; `transform` on any of them;
  * `fill` and `stroke` in black (ink) or white (a knock-out: it takes
    away from the ink under it), `fill-rule`, `stroke-width`,
    `stroke-linecap`, `stroke-linejoin`, `stroke-miterlimit`, inherited
    from the `svg` and `g` elements as in SVG.

Elements are laid over one another in document order, as an SVG
viewer paints them on white, so the file looks in a viewer as the
glyph will: black is ink, white is a hole. Nothing else is read (no
styles, gradients, text or `use`), and an attribute the builder does
not know is an error rather than a guess.

The output is deterministic: the same sources give the same bytes.
"""

import argparse
import math
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path as FsPath

import pathops
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path

ROOT = FsPath(__file__).resolve().parents[2]
ASSETS = ROOT / "crates" / "osmena-chrome" / "assets"

# The drawings' grid and the font's: 128 font units to a grid unit, so
# every half and quarter of a pixel is a whole number of units.
GRID = 16
UPEM = 2048
SCALE = UPEM // GRID

# Seconds since 1904 for 2026-01-01: a fixed date in the `head` table,
# so two builds of the same sources are the same file.
BUILT = 3_849_984_000

SVG = "{http://www.w3.org/2000/svg}"
INK = {"#000", "#000000", "black", "currentcolor"}
VOID = {"#fff", "#ffffff", "white"}
STYLE = {
    "fill": "#000",
    "fill-rule": "nonzero",
    "stroke": "none",
    "stroke-width": "1",
    "stroke-linecap": "butt",
    "stroke-linejoin": "miter",
    "stroke-miterlimit": "4",
}
GEOMETRY = {
    "path": {"d"},
    "circle": {"cx", "cy", "r"},
    "ellipse": {"cx", "cy", "rx", "ry"},
    "rect": {"x", "y", "width", "height", "rx", "ry"},
    "line": {"x1", "y1", "x2", "y2"},
    "polyline": {"points"},
    "polygon": {"points"},
    "g": set(),
}
CAPS = {
    "butt": pathops.LineCap.BUTT_CAP,
    "round": pathops.LineCap.ROUND_CAP,
    "square": pathops.LineCap.SQUARE_CAP,
}
JOINS = {
    "miter": pathops.LineJoin.MITER_JOIN,
    "round": pathops.LineJoin.ROUND_JOIN,
    "bevel": pathops.LineJoin.BEVEL_JOIN,
}
# The offset of a cubic's control points that makes a quarter circle.
KAPPA = 0.5522847498


class GlyphError(Exception):
    pass


def kebab(name):
    """`StarFilled` -> `star-filled`, as serde's kebab-case does."""
    return re.sub(r"(?<!^)(?=[A-Z])", "-", name).lower()


def multiply(m, n):
    """The affine `m` after `n` (both as SVG's a b c d e f)."""
    a, b, c, d, e, f = m
    g, h, i, j, k, l = n
    return (
        a * g + c * h,
        b * g + d * h,
        a * i + c * j,
        b * i + d * j,
        a * k + c * l + e,
        b * k + d * l + f,
    )


def parse_transform(text):
    matrix = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)
    for name, args in re.findall(r"(\w+)\s*\(([^)]*)\)", text or ""):
        v = [float(x) for x in re.split(r"[\s,]+", args.strip()) if x]
        if name == "translate":
            step = (1, 0, 0, 1, v[0], v[1] if len(v) > 1 else 0.0)
        elif name == "scale":
            step = (v[0], 0, 0, v[1] if len(v) > 1 else v[0], 0, 0)
        elif name == "rotate":
            angle = math.radians(v[0])
            cos, sin = math.cos(angle), math.sin(angle)
            step = (cos, sin, -sin, cos, 0, 0)
            if len(v) == 3:
                step = multiply(
                    multiply((1, 0, 0, 1, v[1], v[2]), step), (1, 0, 0, 1, -v[1], -v[2])
                )
        elif name == "matrix":
            step = tuple(v)
        else:
            raise GlyphError(f"a transform this builder does not know: {name}")
        matrix = multiply(matrix, step)
    return matrix


def number(element, name, default=0.0):
    return float(element.get(name, default))


def ellipse(pen, cx, cy, rx, ry):
    kx, ky = rx * KAPPA, ry * KAPPA
    pen.moveTo((cx + rx, cy))
    pen.curveTo((cx + rx, cy + ky), (cx + kx, cy + ry), (cx, cy + ry))
    pen.curveTo((cx - kx, cy + ry), (cx - rx, cy + ky), (cx - rx, cy))
    pen.curveTo((cx - rx, cy - ky), (cx - kx, cy - ry), (cx, cy - ry))
    pen.curveTo((cx + kx, cy - ry), (cx + rx, cy - ky), (cx + rx, cy))
    pen.closePath()


def rounded_rect(pen, x, y, w, h, rx, ry):
    rx, ry = min(rx, w / 2), min(ry, h / 2)
    if rx == 0 or ry == 0:
        pen.moveTo((x, y))
        for point in ((x + w, y), (x + w, y + h), (x, y + h)):
            pen.lineTo(point)
        pen.closePath()
        return
    kx, ky = rx * (1 - KAPPA), ry * (1 - KAPPA)
    pen.moveTo((x + rx, y))
    pen.lineTo((x + w - rx, y))
    pen.curveTo((x + w - kx, y), (x + w, y + ky), (x + w, y + ry))
    pen.lineTo((x + w, y + h - ry))
    pen.curveTo((x + w, y + h - ky), (x + w - kx, y + h), (x + w - rx, y + h))
    pen.lineTo((x + rx, y + h))
    pen.curveTo((x + kx, y + h), (x, y + h - ky), (x, y + h - ry))
    pen.lineTo((x, y + ry))
    pen.curveTo((x, y + ky), (x + kx, y), (x + rx, y))
    pen.closePath()


def points_of(element):
    v = [float(x) for x in re.split(r"[\s,]+", element.get("points", "").strip()) if x]
    return list(zip(v[0::2], v[1::2]))


def outline(element, tag):
    """The element's own geometry, in grid units, as recorded pen calls."""
    pen = RecordingPen()
    if tag == "path":
        parse_path(element.get("d", ""), pen)
    elif tag == "circle":
        r = number(element, "r")
        ellipse(pen, number(element, "cx"), number(element, "cy"), r, r)
    elif tag == "ellipse":
        ellipse(
            pen,
            number(element, "cx"),
            number(element, "cy"),
            number(element, "rx"),
            number(element, "ry"),
        )
    elif tag == "rect":
        rx = element.get("rx", element.get("ry", 0))
        ry = element.get("ry", rx)
        rounded_rect(
            pen,
            number(element, "x"),
            number(element, "y"),
            number(element, "width"),
            number(element, "height"),
            float(rx),
            float(ry),
        )
    elif tag == "line":
        pen.moveTo((number(element, "x1"), number(element, "y1")))
        pen.lineTo((number(element, "x2"), number(element, "y2")))
        pen.endPath()
    else:
        points = points_of(element)
        if points:
            pen.moveTo(points[0])
            for point in points[1:]:
                pen.lineTo(point)
            if tag == "polygon":
                pen.closePath()
            else:
                pen.endPath()
    return pen


def skia_path(recording, close):
    """The recorded outline as a Skia path, scaled up to font units (so
    Skia's own tolerances are far below a pixel); `close` shuts every
    open contour, which is how SVG fills one."""
    path = pathops.Path()
    pen = path.getPen()
    for verb, args in recording.value:
        args = tuple((x * SCALE, y * SCALE) for x, y in args)
        if verb == "endPath" and close:
            pen.closePath()
        else:
            getattr(pen, verb)(*args)
    return path


def in_font_units(matrix):
    """The grid-unit affine `matrix` as one between font-unit spaces."""
    a, b, c, d, e, f = matrix
    return (a, b, c, d, e * SCALE, f * SCALE)


def paint(value, what, source):
    value = value.strip().lower()
    if value == "none":
        return 0
    if value in INK:
        return 1
    if value in VOID:
        return -1
    raise GlyphError(f"{source}: {what}={value!r}; only black, white and none are read")


def lay(ink, shape, sign):
    if sign > 0:
        return pathops.op(ink, shape, pathops.PathOp.UNION)
    return pathops.op(ink, shape, pathops.PathOp.DIFFERENCE)


def walk(element, style, matrix, ink, source):
    tag = element.tag.removeprefix(SVG)
    if tag not in GEOMETRY and tag != "svg":
        raise GlyphError(f"{source}: <{tag}> is not read")
    known = set(STYLE) | GEOMETRY.get(tag, set()) | {"transform"}
    if tag == "svg":
        known |= {"viewBox", "width", "height"}
    for name in element.keys():
        if name not in known:
            raise GlyphError(f"{source}: <{tag} {name}=...> is not read")
    style = dict(style)
    for name in STYLE:
        if name in element.attrib:
            style[name] = element.get(name)
    matrix = multiply(matrix, parse_transform(element.get("transform")))
    if tag in ("svg", "g"):
        for child in element:
            ink = walk(child, style, matrix, ink, source)
        return ink
    recording = outline(element, tag)
    placed = in_font_units(matrix)
    fill = paint(style["fill"], "fill", source)
    if fill and tag != "line":
        shape = skia_path(recording, close=True)
        if style["fill-rule"] == "evenodd":
            shape.fillType = pathops.FillType.EVEN_ODD
        shape.simplify()
        ink = lay(ink, shape.transform(*placed), fill)
    stroke = paint(style["stroke"], "stroke", source)
    if stroke:
        shape = skia_path(recording, close=False)
        shape.stroke(
            float(style["stroke-width"]) * SCALE,
            CAPS[style["stroke-linecap"]],
            JOINS[style["stroke-linejoin"]],
            float(style["stroke-miterlimit"]),
        )
        shape.convertConicsToQuads()
        shape.simplify()
        ink = lay(ink, shape.transform(*placed), stroke)
    return ink


def glyph_of(svg):
    """The SVG file as a TrueType glyph and its ink's bounds in grid
    units (left, top, right, bottom)."""
    root = ET.parse(svg).getroot()
    if root.get("viewBox", "").split() != ["0", "0", str(GRID), str(GRID)]:
        raise GlyphError(f"{svg.name}: the viewBox must be 0 0 {GRID} {GRID}")
    ink = walk(root, STYLE, (1.0, 0.0, 0.0, 1.0, 0.0, 0.0), pathops.Path(), svg.name)
    ink.convertConicsToQuads()
    ink.simplify(fix_winding=True)
    if not list(ink.contours):
        raise GlyphError(f"{svg.name}: nothing is drawn")
    left, top, right, bottom = (v / SCALE for v in ink.bounds)
    # Font space has y up. Skia leaves the outer contours running
    # anticlockwise as it sees them, y down; the flip turns them
    # clockwise, which is the way TrueType wants them.
    glyph_pen = TTGlyphPen(None)
    flipped = TransformPen(
        Cu2QuPen(glyph_pen, max_err=0.5, reverse_direction=False),
        (1, 0, 0, -1, 0, UPEM),
    )
    ink.draw(flipped)
    return glyph_pen.glyph(), (left, top, right, bottom)


def variants(source, pattern):
    """`{file name: code point}` for the variants `pattern` finds in the
    Rust file `source`."""
    found = re.findall(pattern, source.read_text(encoding="utf-8"))
    if not found:
        raise GlyphError(f"{source}: no variants found; has the code moved?")
    return {kebab(name): int(code, 16) for name, code in found}


def build(family, directory, codes, output, notice, licence):
    files = {path.stem: path for path in sorted(directory.glob("*.svg"))}
    missing = sorted(set(codes) - set(files))
    spare = sorted(set(files) - set(codes))
    if missing or spare:
        raise GlyphError(
            f"{directory.name}: no drawing for {missing}; no variant for {spare}"
        )
    names = sorted(codes, key=lambda name: codes[name])
    glyphs = {".notdef": TTGlyphPen(None).glyph()}
    bounds = {}
    for name in names:
        glyphs[name], bounds[name] = glyph_of(files[name])
    fb = FontBuilder(UPEM, isTTF=True)
    fb.setupGlyphOrder([".notdef"] + names)
    fb.setupCharacterMap({codes[name]: name for name in names})
    fb.setupGlyf(glyphs)
    table = fb.font["glyf"]
    fb.setupHorizontalMetrics(
        {name: (UPEM, table[name].xMin if name in bounds else 0) for name in glyphs}
    )
    # The line is the em square and nothing more, the baseline along
    # its foot: a glyph set at size s fills exactly an s by s box.
    fb.setupHorizontalHeader(ascent=UPEM, descent=0)
    fb.setupNameTable(
        {
            "familyName": family,
            "styleName": "Regular",
            "copyright": notice,
            "licenseDescription": licence,
        }
    )
    fb.setupOS2(
        sTypoAscender=UPEM,
        sTypoDescender=0,
        sTypoLineGap=0,
        usWinAscent=UPEM,
        usWinDescent=0,
    )
    fb.setupPost()
    fb.font["head"].created = BUILT
    fb.font["head"].modified = BUILT
    fb.font.recalcTimestamp = False
    fb.save(output)
    print(f"{output.relative_to(ROOT)}: {len(names)} glyphs, {output.stat().st_size} bytes")
    return names, bounds


def preview(directory, font, names, codes, discs):
    """A sheet of every glyph at its natural size, magnified beside it,
    and at twice the size; for the avatars on the discs the chrome
    draws them on."""
    from PIL import Image, ImageDraw, ImageFont

    ground, ink, disc = (250, 248, 244), (24, 24, 24), (229, 214, 194)
    columns = 8
    cell_w, cell_h = 250, 150
    rows = math.ceil(len(names) / columns)
    sheet = Image.new("RGB", (columns * cell_w, rows * cell_h), ground)
    draw = ImageDraw.Draw(sheet)
    label = ImageFont.load_default()

    def tile(size, radius):
        # Drawn eight times too large and averaged down: FreeType hints
        # a font that brings no hints of its own, which the chrome's
        # renderer does not, and at this size the hinter moves nothing
        # by more than an eighth of a pixel.
        over = 8
        side = 2 * radius if discs else size + 8
        image = Image.new("RGB", (side * over, side * over), ground)
        d = ImageDraw.Draw(image)
        if discs:
            d.ellipse([0, 0, side * over - 1, side * over - 1], fill=disc)
        d.text(
            ((side - size) // 2 * over, (side - size) // 2 * over),
            chr(codes[name]),
            fill=ink,
            font=ImageFont.truetype(str(font), size * over),
            anchor="la",
        )
        return image.resize((side, side), Image.BOX)

    for index, name in enumerate(names):
        x, y = (index % columns) * cell_w + 8, (index // columns) * cell_h + 6
        draw.text((x, y), name, fill=(90, 90, 90), font=label)
        small = tile(16, 10)
        sheet.paste(small, (x, y + 18))
        sheet.paste(
            small.resize((small.width * 6, small.height * 6), Image.NEAREST), (x + 28, y + 18)
        )
        sheet.paste(tile(32, 20) if not discs else tile(26, 16), (x + 156, y + 18))
        if discs:
            sheet.paste(tile(45, 28), (x + 192, y + 18))
    directory.mkdir(parents=True, exist_ok=True)
    out = directory / f"{font.stem}.png"
    sheet.save(out)
    print(f"{out}: the preview")


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "only", nargs="?", choices=["icons", "avatars"], help="build just this font"
    )
    parser.add_argument("--preview", type=FsPath, help="also write a sheet of each font here")
    args = parser.parse_args()
    sets = [
        (
            "Osmena Icons",
            ASSETS / "icons",
            variants(
                ROOT / "crates/osmena-chrome/src/icons.rs",
                r"Icon::(\w+) => '\\u\{([0-9A-Fa-f]+)\}'",
            ),
            ASSETS / "fonts" / "OsmenaIcons.ttf",
            "Copyright 2026 the osmena contributors",
            "MIT OR Apache-2.0, as the repository this font is built in",
            False,
        ),
        (
            "Osmena Avatars",
            ASSETS / "avatars",
            variants(
                ROOT / "crates/osmena-core/src/profile.rs",
                r"Avatar::(\w+) => \"\\u\{([0-9A-Fa-f]+)\}\"",
            ),
            ASSETS / "fonts" / "OsmenaAvatars.ttf",
            "Copyright 2026 the osmena contributors. The whale is from Noto Emoji, "
            "Copyright 2013 Google LLC.",
            "SIL Open Font License, Version 1.1 (LICENSE-OsmenaAvatars.txt)",
            True,
        ),
    ]
    try:
        for family, directory, codes, output, notice, licence, discs in sets:
            if args.only not in (None, directory.name):
                continue
            names, _ = build(family, directory, codes, output, notice, licence)
            if args.preview:
                preview(args.preview, output, names, codes, discs)
    except GlyphError as error:
        sys.exit(f"error: {error}")


if __name__ == "__main__":
    main()
