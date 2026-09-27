#!/usr/bin/env python3
"""Builds media/overseer-mark.woff, a one-glyph icon font of Overseer's single-colour mark (AC-142).

VS Code's status bar only shows product icons ($(name)), and an extension adds one through
contributes.icons with a font. The glyph is drawn from docs/design/brand/overseer-mark.svg (a 24-unit
square, one path) at U+E001, on the same grid as the codicons (units per em = the square, baseline
at the bottom, no descent). Needs fontTools (`pip install fonttools`). Run it again whenever the mark
changes: `python3 extension/design/build-mark-font.py`.
"""
import os
import re
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.cu2quPen import Cu2QuPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.svgLib.path import parse_path

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, '..', '..'))
SVG = os.path.join(ROOT, 'docs', 'design', 'brand', 'overseer-mark.svg')
OUT = os.path.join(HERE, '..', 'media', 'overseer-mark.woff')
UPM, BOX, CODEPOINT = 1000, 24.0, 0xE001

svg = open(SVG, encoding='utf-8').read()
d = re.search(r'\sd="([^"]+)"', svg).group(1)
tt = TTGlyphPen(None)
# SVG y grows down; font y grows up with the baseline at the bottom of the square.
pen = TransformPen(Cu2QuPen(tt, max_err=0.5, reverse_direction=True), (UPM / BOX, 0, 0, -UPM / BOX, 0, UPM))
parse_path(d, pen)
glyph = tt.glyph()

fb = FontBuilder(UPM, isTTF=True)
fb.setupGlyphOrder(['.notdef', 'overseer-mark'])
fb.setupCharacterMap({CODEPOINT: 'overseer-mark'})
fb.setupGlyf({'.notdef': TTGlyphPen(None).glyph(), 'overseer-mark': glyph})
fb.setupHorizontalMetrics({'.notdef': (UPM, 0), 'overseer-mark': (UPM, glyph.xMin)})
fb.setupHorizontalHeader(ascent=UPM, descent=0)
fb.setupNameTable({'familyName': 'Overseer Mark', 'styleName': 'Regular'})
fb.setupOS2(sTypoAscender=UPM, sTypoDescender=0, usWinAscent=UPM, usWinDescent=0)
fb.setupPost()
fb.font.flavor = 'woff'
fb.save(OUT)
print('wrote', os.path.relpath(OUT, ROOT))
