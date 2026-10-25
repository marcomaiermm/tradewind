# Tradewind application icon

Original artwork for this project, licensed like the code (MIT OR Apache-2.0).

| File | Use |
|---|---|
| `tradewind.svg` | Source, installed as the scalable hicolor icon |
| `icon-16.png`, `icon-32.png` | Hand-tuned small sizes |
| `icon-48.png`, `icon-128.png`, `icon-256.png` | Rendered from the SVG |
| `icon-256.rgba` | Iced/X11 window icon |
| `icon-32.rgba`, `icon-48.rgba` | StatusNotifierItem tray pixmaps (converted to ARGB at the desktop boundary) |

All PNG sizes are installed into the hicolor theme as `tradewind`. Wayland
titlebars and taskbars resolve the `tradewind` application ID through desktop
metadata. The current winit Wayland backend ignores embedded window pixels.

A direct launch without an installed menu entry creates a hidden fallback
`$XDG_DATA_HOME/applications/tradewind.desktop` pointing at a copy of the
256 px PNG in `$XDG_DATA_HOME/tradewind/icons/app.png`, so desktops can show
the icon. Installed or custom entries are never overwritten.

Regenerate the rendered sizes after editing the SVG:

```sh
cd data/icons/app
for s in 48 128 256; do rsvg-convert -w $s -h $s tradewind.svg -o icon-$s.png; done
python3 -c 'from PIL import Image
for s in (32, 48, 256): open(f"icon-{s}.rgba", "wb").write(Image.open(f"icon-{s}.png").convert("RGBA").tobytes())'
```
