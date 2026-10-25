# Iced software-renderer clipping and damage fixes

This directory contains the published `iced_tiny_skia` 0.14.0 source
(MIT, see LICENSE), from the Cargo registry. `Cargo.toml` uses this local copy
through `[patch.crates-io]`. No new dependency or application crate is added.

## Text clipping

The first change is the `Text::Cached` branch in `src/engine.rs`.
Upstream compares the supplied text clip rectangle to the layer bounds and
omits the clipping mask if that rectangle is contained. The painted glyphs can
extend outside this rectangle (partially visible or long PickList options), so
scrolling draws text over neighboring controls. Always apply the intersection
of text and layer clipping, then restore the layer mask for subsequent drawing.

Regression evidence: `ui::auction::scroll_tests::headers_emit_column_sorts_and_native_menu_selects_scrolled_options`
opens Iced's actual PickList, scrolls it, verifies every changed pixel stays
inside the menu and selects the last option. It fails on the registry version
with text painted outside the menu. It passes with this patch. Other tests
check body clipping at 1× and 1.25× and DE/EN menu alignment.

## Partial redraws (dropdown flicker)

The window compositor redraws only the damage since the swapchain buffer it
gets back was last shown. Three defects made those partial frames differ from
full ones, so reused buffers alternated and open dropdowns flickered:

- `Engine::draw_quad` drew shadows without the clip mask, blending the whole
  menu shadow again over pixels outside the damage on every hover. It now
  clips shadows and skips a quad only when neither it nor its shadow is in view.
- `Layer::damage` covered a changed quad but not its shadow (`src/layer.rs`).
- Cached text (e.g. a PickList's selected label) paints from its alignment
  point, so a vertically centred label starts half a line above its bounds.
  `Layer::damage` covered only the lower half, leaving the top half of an old
  label under a new one (`painted_bounds` in `src/layer.rs`).
- Fractional damage and layer edges anti-aliased content only when a mask was
  in use. `Renderer::draw` now grows both to whole pixels (`snap` in `src/lib.rs`).

Regression evidence: `ui::auction::scroll_tests::partial_redraws_of_open_menus_match_full_redraws`
replays the compositor's damage path for one to three buffers at 1× and 1.25×
while a dropdown opens, is hovered row by row and closes, and compares every
frame with a full redraw (2 levels of rounding tolerance between the masked and
unmasked pipelines). On the registry version the first hover leaves 16,676
pixels off by up to 26 levels.

`changed_select_labels_are_damaged_where_they_paint` covers the label damage,
and `hovering_controls_beside_the_version_icon_matches_full_redraws` replays
hover states with an SVG beside the selects.

Remove these patches when an upstream version passes these tests unmodified.
Do not modify the Cargo registry cache or replace standard dropdown widgets
with bespoke menus to work around this renderer defect.
