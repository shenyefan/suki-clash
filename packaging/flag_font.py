#!/usr/bin/env python3
"""Build the flag-only font from Mozilla's Twemoji COLR v0.7.0.

Requires fonttools==4.68.0. The color layers stay unchanged. Composite base
outlines give GPUI/DirectWrite a non-empty raster bound for those layers.
"""
import argparse
from pathlib import Path

from fontTools import subset
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont


def build(source: Path, destination: Path):
    font = TTFont(source, recalcTimestamp=False)
    options = subset.Options()
    options.glyph_names = True
    options.layout_features = ["*"]
    subsetter = subset.Subsetter(options=options)
    subsetter.populate(unicodes=[
        *range(0x1F1E6, 0x1F200), 0x1F3F4,
        *range(0xE0061, 0xE007B), 0xE007F,
    ])
    subsetter.subset(font)
    glyphs = font["glyf"]
    for name, layers in font["COLR"].ColorLayers.items():
        pen = TTGlyphPen(glyphs)
        for layer in layers:
            pen.addComponent(layer.name, (1, 0, 0, 1, 0, 0))
        glyphs[name] = pen.glyph()
    names = {
        1: "Suki Flag Emoji", 2: "Regular", 3: "SukiFlagEmoji-0.7.0",
        4: "Suki Flag Emoji", 6: "SukiFlagEmoji", 16: "Suki Flag Emoji", 17: "Regular",
    }
    for record in font["name"].names:
        if record.nameID in names:
            record.string = names[record.nameID].encode(record.getEncoding())
    font.save(destination)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    build(args.source, args.destination)
