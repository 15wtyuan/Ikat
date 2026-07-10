---
name: loomgui-editor
description: |
  Generate LoomGUI fence-compliant UI (HTML+CSS) for game dashboards/panels.
  Uses flex-only layout, tag whitelist (div/span/img/button), no grid/margin-spacing.
  After generating, run `loom-pkg build <workspace>` to validate + pack into .pkg.bin.
---

# LoomGUI Editor

Generate fence-compliant game UI (HTML+CSS) for LoomGUI, validated and packed via `loom-pkg build`.

## Workflow

1. **Understand the workspace**: Read `loom.workspace.json` in the workspace root. It defines packages (HTML/CSS to build), atlases (image directories to pack), and fonts. Use this to know what already exists and where to add new components.

2. **Generate HTML+CSS** according to the designer's prompt:
   - **Elements**: Only `div`, `span` (+ raw text), `img`, `button`.
   - **Layout**: Flexbox only. `div` always defaults to `flex-direction: column` (not browser block flow). Use `gap` for child spacing (not margin — Chrome collapses margins, LoomGUI doesn't).
   - **Inline mix is a compile error**: A single element cannot contain both text and child elements (e.g. `<div>text<span/></div>` is illegal). Put text in a wrapper `<span>`.
   - Place HTML files under the package's `dirs` directory (e.g. `ui/showcase/my-panel.html`).
   - `<img src="...">` is relative to the HTML file (browser-native).
   - If you add images that aren't referenced by any HTML `<img>`, add their directory to an atlas `dirs` so they get packed into an atlas texture (for runtime dynamic icons).

3. **Build and validate**:
   ```bash
   loom-pkg build <workspace-root>
   ```
   - **Non-zero exit = fence violation or asset error**. Read the stderr output carefully — it tells you exactly what went wrong (missing image, atlas conflict, unsupported tag, inline mix, etc.).
   - **Self-correct**: Fix the HTML/CSS based on the error message, then re-run `loom-pkg build`.
   - **Zero exit = success**. Artifacts are in `{output_dir}/`:
     - `ui/*.pkg.bin` — packaged UI components
     - `atlas/{name}.png` + `{name}.atlas.json` — texture atlases
     - `fonts/*.bytes` — fonts
     - `loom.runtime.json` — runtime manifest

4. **Report**: Tell the designer the artifact paths and that the game engine backend loads them from the output directory.

## Fence rules (hard constraints)

These are the key rules. The authoritative source is `loomgui_core/tests/fence_contract.rs` in the LoomGUI repository.

### Element whitelist
Only: `div`, `span` (+ raw text), `img`, `button`. Other tags (video, input, p, ul, etc.) cause a **parse error** — the build fails.

### CSS layout (supported)
- `display: flex` | `none` — **no grid** (silently ignored, preview will deceive you)
- `flex-direction`, `flex-wrap`, `gap` / `row-gap` / `column-gap`
- `justify-content`, `align-items`, `align-self`, `flex` (grow/shrink/basis), `order`
- `aspect-ratio`
- `width` / `height` / `min-*` / `max-*` (px / % / auto)
- `padding`, `margin`, `border-width`
- Use `gap` for child spacing, not margin
- `position: absolute` (with `top`/`right`/`bottom`/`left`); **no** `position: fixed` / `sticky`, **no** `float`, **no** `align-content`
- **No `inset` shorthand** — use `top`/`right`/`bottom`/`left` explicitly

### CSS visual (supported)
- `background-color`, `background-image` (url), `background-size` (cover/contain/100%, single value only)
- `border-radius`, `border` (shorthand: `<width> <style>? <color>?`), `border-color`, `border-width`
- `opacity`
- `overflow` / `overflow-x` / `overflow-y`
- `color`, `font-size` (px), `font-family`, `font-weight`
- `text-align`, `line-height`, `letter-spacing`, `white-space: nowrap`
- `transform` (translate/rotate/scale — no skew/matrix)
- `pointer-events`
- `filter` (grayscale/brightness/contrast/saturate/hue-rotate/invert/sepia)
- `border-image-slice` (9-slice)
- **Not supported** (silently ignored): `clip-path`, `background-position`, `background-repeat`, `transform-origin`, `font-style`, `cursor`

### Selectors
- Pseudo-classes: `:hover`, `:active`, `:disabled`, `:focus`
- Combinators: tag, class, id, descendant, child (`>`), grouping (`,`)
- Attribute selectors: `[attr]`, `[attr="val"]`
- **Not supported**: `+`, `~`, `*`, `:nth-child()`, `:not()`

### The `div` default
Every `<div>` is a flex container (`flex-direction: column`). There is no browser block/inline flow — only flex items participate in layout. This means text inside a `<div>` without a wrapping `<span>` participates as a flex item, but **inline mix** (text + element + text in one container) is a compile error.

### Preview trust
Trust: flex, gap, color, px units, background-image, position:absolute.
**Don't trust**: margin collapsing, pixel-exact text wrapping, `display:grid` (renders as flex in LoomGUI), `@media` (ignored).
Rule: "trust the fence rules, not the preview."
