# Nova Sound app icon (placeholder)

**A nightingale singing**, as an engraving-style head-and-shoulders portrait in the Nova Foods Apps'
owl-template framing (craftrules `standards/icon-design.md`): a full-bleed colour field, the bird
in three-quarter view with its bill open and its eye on the viewer, its breast and wing running off
the bottom and right edges, three song arcs leaving the bill.

This is a **placeholder** until the owner draws Nova Sound's icon in Nova Foods like the other apps'
icons. Replace `icon.svg` and run `packaging/icons.sh`; nothing else needs to change.

## Palette

Exactly three colours:

| Colour | Hex | Used for |
|---|---|---|
| Ink | `#0b0b0c` | Line work and contour |
| Paper | `#efe9dc` | The bird and the song arcs |
| Nova Sound teal (app colour) | `#14a9c4` | The full-bleed background field |

## Geometry

A 512 × 512 viewBox, clipped to a rounded square with `rx=112`. Windows and Linux use the full
tile. The macOS icons (`novasound.icns`, `novasound-macos-512.png`) sit on Apple's grid: an
824/1024 body with a transparent margin.

## Provenance and licence

Drawn from scratch as hand-placed SVG paths by the Nova Sound contributors; no source image was
traced or copied. MIT OR Apache-2.0, like the code (see `ATTRIBUTION.md`).

## Files

| File | What it is |
|---|---|
| `icon.svg` | The canonical vector |
| `novasound-1024.png` | 1024 px render |
| `novasound-macos-512.png` | Runtime window/Dock icon on macOS (Apple grid margin) |
| `novasound.icns` | macOS bundle icon |
| `novasound.ico` | Windows icon (16–256 px), embedded in `novasound.exe` and used by the MSI |
| `hicolor/<size>x<size>/apps/br.com.novafoods.novasound.png` | Linux/FreeBSD icon theme, 16–512 px, plus `scalable/` |

Regenerate everything with `packaging/icons.sh` (needs `resvg`, and `iconutil` on macOS for the
`.icns`).
