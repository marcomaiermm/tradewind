# Local interface icons

The original TSM application/tray icon and extraction provenance are documented
in [app/README.md](app/README.md).

`hot-deal.svg` uses the original [Lucide Flame](https://lucide.dev/icons/flame)
path from Lucide 1.8.0, with the stroke color set to `#D35419`.
Copyright and terms are in `LICENSE.lucide` (ISC).

`hot-deal.rgba` is its 64 × 64, straight RGBA render, displayed at 18 px by
Iced. The existing image renderer reads it without adding an SVG or PNG codec.
Regenerate from the checked-in SVG with librsvg and Pillow:

```sh
rsvg-convert data/icons/hot-deal.svg -o /tmp/tsm-hot-deal.png
python3 - <<'PY'
from pathlib import Path
from PIL import Image
Path('data/icons/hot-deal.rgba').write_bytes(
    Image.open('/tmp/tsm-hot-deal.png').convert('RGBA').tobytes()
)
PY
```

The `sort-ascending.svg` and `sort-descending.svg` chevrons use Lucide 1.8.0
`chevron-up` / `chevron-down` paths with a `#858585` stroke. Their 32 × 32 RGBA
renders are displayed at 12 px next to the active table heading. The original
Lucide and Feather license notices are retained in `LICENSE.lucide`. Regenerate
with the same commands above, substituting each filename and preserving 32 × 32.
`sort-neutral.svg` uses Lucide’s `chevrons-up-down` with the same 32 px source,
12 px display and stroke. It marks sortable columns before a sort is chosen.
Dropdown arrows come from Iced's standard PickList handle.
