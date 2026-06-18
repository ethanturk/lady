# Commit-graph rendering: Canvas 2D (hybrid)

The commit graph renders its **lanes + edges to an HTML `<canvas>` (2D)**, chosen over virtualized SVG/DOM for performance headroom and smooth scroll on dense histories. Lane-layout data comes from Rust (`lady-graph`); the renderer consumes a row-window interface and is kept **swappable** (canvas/WebGL/SVG behind one contract).

**Hybrid refinement:** commit-row **text, refs, and metadata render as virtualized DOM** aligned to the canvas rows — so theming, text selection, and accessibility come for free on the text layer, while canvas handles only the dense edge/node drawing.

**Why:** matches Fork's buttery graph feel; canvas gives the most control over edge routing and redraw cost.

**Consequences (canvas is opaque to CSS and screen readers):**
- **Hit-testing** is manual: map mouse `(x,y)` → row / ref / edge via a per-frame spatial index.
- **Theming pipeline:** pass theme color tokens (from CSS vars) into draw calls and re-render on theme change — no CSS styling of canvas.
- **Accessibility:** maintain a parallel ARIA/DOM structure + keyboard nav (the hybrid DOM text layer covers most of this).
- **Retina:** handle `devicePixelRatio` scaling to stay sharp.
