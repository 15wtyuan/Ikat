---
name: loomgui-editor
description: |
  Generate LoomGUI fence-compliant UI (HTML+CSS) for game dashboards/panels.
  Uses standard HTML/CSS subset with 30 fence tags, schema-driven validation.
  After generating, run `loom-pkg build <workspace>` to validate + pack into .pkg.bin.
---

# LoomGUI Editor

Generate fence-compliant game UI (HTML+CSS) for LoomGUI, validated and packed via `loom-pkg build`.

## Core principle

**Tags determine stable object types; CSS grants behavior capabilities but never changes types.** This means you write standard HTML, and the fence validates it against a schema. AI can predict the rendering result from the HTML alone — that is the primary design criterion.

## Workflow

1. **Understand the workspace**: Read `loom.workspace.json` in the workspace root. It defines packages (HTML/CSS to build), atlases (image directories to pack), and fonts. Use this to know what already exists and where to add new components.

2. **Generate HTML+CSS** according to the designer's prompt using standard HTML semantics:
   - Use any of the 30 fence tags (see list below). Unknown tags cause a build error.
   - `div`, `header`, `nav`, `p`, `ul`, `ol`, `li`, `option` are block-level by default (standard HTML).
   - `span`, `strong`, `em`, `label`, `button`, `a`, `img`, `input`, etc. are inline by default.
   - `display:flex` defaults to `flex-direction:row` (standard CSS). Use `flex-direction:column` for vertical stacking.
   - Use `gap` for child spacing (not margin — Chrome collapses margins, LoomGUI doesn't).
   - `overflow:auto` or `overflow:scroll` enables scroll containers (no custom tag needed).
   - `display:grid` is a build error — it is NOT silently downgraded to flex.
   - Place HTML files under the package's `dirs` directory (e.g. `ui/showcase/my-panel.html`).
   - `<img src="...">` is relative to the HTML file (browser-native).
   - If you add images that aren't referenced by any HTML `<img>`, add their directory to an atlas `dirs` so they get packed.

3. **Build and validate**:
   ```bash
   loom-pkg build <workspace-root>
   ```
   - **Non-zero exit = fence violation or asset error**. The packer collects ALL diagnostics in one pass and reports them together (file/line/column). Read the output carefully and fix all errors before re-running.
   - **Self-correct**: Fix the HTML/CSS based on the error messages, then re-run `loom-pkg build`.
   - **Zero exit = success**. Artifacts are in `{output_dir}/`:
     - `ui/*.pkg.bin` — packaged UI components
     - `atlas/{name}.png` + `{name}.atlas.json` — texture atlases
     - `fonts/*.bytes` — fonts
     - `loom.runtime.json` — runtime manifest

4. **Report**: Tell the designer the artifact paths and that the game engine backend loads them from the output directory.

## Supported tags (30 total)

### Document shell (consumed at build time)
`html`, `head`, `body`, `title`, `meta`, `style`, `link`

### Runtime tags (23)

| Tag | Type | Default display |
|---|---|---|
| `div` `header` `nav` | Container | block |
| `p` | TextBlock | block |
| `span` `strong` `em` | TextElement | inline |
| `br` | LineBreak | inline (void) |
| `label` | Label | inline |
| `button` | Button | inline |
| `a` | Link | inline |
| `img` | Image | inline (void) |
| `canvas` | Canvas | inline |
| `input` | varies by `type` | inline (void) |
| `textarea` | TextArea | inline |
| `select` | Dropdown | inline |
| `option` | OptionItem | block |
| `progress` | ProgressBar | inline |
| `ul` `ol` | ListView | block |
| `li` | ListItem | block |
| `template` | Template | none (inert) |
| `slot` | Slot | inline |

Custom elements with a hyphen (e.g. `<my-widget>`) are recognized as CustomElement.

### `input[type]` dispatch

| type | Object type |
|---|---|
| `text` (default), `password`, `search` | TextField |
| `number` | NumberField |
| `range` | Slider |
| `checkbox` | Toggle |
| `radio` | RadioButton |

## CSS rules

- Use only fence-recognized CSS properties. Unknown properties cause build errors (not silent ignoring).
- Supported: width/height, min-/max- sizes, display (block/flex/none/inline), flexbox properties, gap, padding, margin, border, border-radius, background-*, opacity, overflow, color, font-*, text-align, line-height, letter-spacing, white-space, transform, filter, box-shadow, text-shadow, transition, and more.
- **Not supported** (build error): `display:grid`, `cursor`, `clip-path`, `background-position`, `background-repeat`, `float`, `position:fixed/sticky`.

## What causes build errors

- Unknown tag (not in the 30-tag list, not a hyphenated custom element)
- Unknown attribute on an element
- Unknown CSS property name or invalid keyword value
- Invalid content model (e.g. `<div>` inside `<span>`)
- Duplicate `id` in the same template
- `label[for]`, `aria-controls`, or `aria-labelledby` pointing to a non-existent ID

## Preview trust

Trust: flex layout, gap, color, px units, background-image, position:absolute, border-radius.
Don't trust: margin collapsing, pixel-exact text wrapping, `display:grid` (build error in LoomGUI).
Rule: "trust the fence rules, not the preview."
