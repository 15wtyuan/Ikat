# LoomGUI Workspace

This is a LoomGUI workspace directory. The workspace is the single source of truth for your game UI: HTML/CSS components, image assets for atlases, fonts, and the build configuration in `loom.workspace.json`.

## Workspace structure

```
workspace/
  loom.workspace.json       -> The configuration file (see below)
  ui/                       -> HTML/CSS components (one .pkg.bin per package)
    showcase/
      main.html
      main.css
  assets/                   -> Image source files for atlases (PNG only)
    icons/
      home.png
  fonts/                    -> Font files (.ttf / .otf / .ttc)
    NotoSansSC.ttc
  dist/                     -> Default output directory (configurable via output_dir)
```

## `loom.workspace.json` fields

### Top-level

| Field | Type | Description |
|---|---|---|
| `version` | u32 | Config format version (currently `1`) |
| `output_dir` | string | Path relative to workspace root where build artifacts go |
| `packages` | PackageCfg[] | UI packages to build (one `.pkg.bin` per package) |
| `atlases` | AtlasCfg[] | Texture atlases to pack from image sources |
| `fonts` | FontCfg[] | Fonts to include in the runtime bundle |

All paths are relative to the workspace root and use forward slashes.

### `packages[]`

| Field | Type | Description |
|---|---|---|
| `name` | string | Package name (produces `{name}.pkg.bin`) |
| `dirs` | string[] | Directories to collect `.html` files from (relative to workspace root) |
| `html` | string[] | Empty `[]` = auto-scan (packer scans `dirs` top-level `.html` files); non-empty = explicit mode (build only these listed files). Set back to `[]` to restore auto-scan. |

### `atlases[]`

| Field | Type | Default | Description |
|---|---|---|---|
| `name` | string | (required) | Atlas name (produces `{name}.png` + `{name}.atlas.json`) |
| `default` | bool | `false` | Whether images not explicitly assigned to any atlas go here |
| `standalone` | bool | `false` | Whether each image gets its own page (for very large images) |
| `dirs` | string[] | (required) | Directories to recursively scan for `.png` files |
| `max_size` | u32 | `2048` | Maximum atlas page size in pixels (powers of 2 recommended) |
| `padding` | u32 | `4` | Padding between sprites in pixels (prevents bleed) |

### `fonts[]`

| Field | Type | Default | Description |
|---|---|---|---|
| `family` | string | (required) | Font family name (matches CSS `font-family`) |
| `file` | string | (required) | Path to font file relative to workspace root |
| `default` | bool | `false` | Whether this is the default font (used when no family is specified) |
| `fallback` | bool | `false` | Whether this font is used as a fallback (for missing glyphs) |

## `img src` convention

`<img src="...">` in HTML files is **relative to the HTML file itself** (browser-native behavior). This means:

- `ui/showcase/main.html` with `<img src="home.png">` resolves to `ui/showcase/home.png`
- `<img src="images/x.png">` resolves to `ui/showcase/images/x.png`

The packer converts these to **sprite keys** (image path relative to workspace root, e.g. `ui/showcase/home.png`). Sprite keys are globally unique across the entire workspace.

You can also reference images from `assets/` atlas directories: `<img src="../../assets/icons/home.png">` resolves to `assets/icons/home.png`.

## Building

One command:

```bash
loom-pkg build <path-to-this-workspace>
```

This reads `loom.workspace.json`, processes all packages and atlases, and produces:
- `{output_dir}/ui/*.pkg.bin` - packaged UI components
- `{output_dir}/atlas/{name}.png` + `{name}.atlas.json` - texture atlases with sprite UV maps
- `{output_dir}/fonts/*.bytes` - font binary copies
- `{output_dir}/loom.runtime.json` - runtime bootstrap manifest

## Error reporting

The packer reports actionable build errors:

- **Missing image**: an `<img src="...">` references an image not found in any atlas directory, and no `default` atlas exists to catch it -> error listing the orphaned sprite keys
- **Atlas conflict**: the same image path is covered by more than one atlas -> error listing the conflict
- **Atlas overflow**: images don't fit within the atlas `max_size` pages -> error with size details
- **Missing font**: a font `file` path cannot be found -> error with the path
- **Fence violation**: HTML uses tags, attributes, or CSS outside the LoomGUI fence -> compile-time error listing all violations with file/line/column (see below)

## Fence (supported HTML/CSS subset)

LoomGUI validates HTML/CSS against a schema-driven fence at build time. The authoritative specification is in the LoomGUI repository at `docs/design/fence.md`, and the machine-readable schema lives in `crates/fence/src/schema/`. The loomgui-editor skill (`{{SKILLS_DIR}}/loomgui-editor/SKILL.md`) summarizes the key rules.

**Key principle**: Tags determine stable object types; CSS grants behavior capabilities but never changes types. Fence violations cause **build-time errors** (not silent degradation). The packer collects all diagnostics in one pass and reports them together.

### Supported tags (30 total)

**Document shell** (consumed at build time, not in runtime tree): `html`, `head`, `body`, `title`, `meta`, `style`, `link`

**Runtime tags** (23): `div`, `header`, `nav`, `p`, `span`, `strong`, `em`, `br`, `label`, `button`, `a`, `img`, `canvas`, `input`, `textarea`, `select`, `option`, `progress`, `ul`, `ol`, `li`, `template`, `slot`

Custom elements with a hyphen (e.g. `<my-widget>`) are recognized as CustomElement.

### Layout semantics (standard HTML/CSS)

- `div`, `header`, `nav`, `p`, `ul`, `ol`, `li`, `option` default to `display:block`.
- `span`, `strong`, `em`, `label`, `button`, `a`, `img`, `input`, etc. default to `display:inline`.
- `display:flex` defaults to `flex-direction:row` (standard CSS default). Use `flex-direction:column` for vertical stacking.
- `display:block` and `display:flex` select internal layout strategy without changing the node type.
- `overflow:auto` or `overflow:scroll` enables scroll behavior.
- Use `gap` for child spacing (not margin).
- `display:grid` is NOT supported - it is a build-time error, not silently downgraded to flex.

### CSS properties

The fence recognizes a whitelist of CSS properties (width/height, flexbox, padding/margin, border, background, opacity, overflow, color/font/text, transform, filter, transitions, etc.). Properties outside the whitelist cause build-time errors. See `docs/design/fence.md` for the complete list.

### What causes build errors

- Unknown tag (not in the 30-tag fence, not a custom element with hyphen)
- Unknown attribute (not a global attr, not a tag-specific structural/content attr)
- Unknown CSS property or invalid keyword value (e.g. `display:grid`)
- Invalid content model (e.g. `<div>` inside `<span>`)
- Duplicate `id` in the same template scope
- Missing `label[for]`, `aria-controls`, or `aria-labelledby` target

All errors are collected and reported together with file name, line, and column.
