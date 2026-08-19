# LoomGUI Workspace

This directory is a LoomGUI UI workspace: the single source of truth for your game's UI. You write standard HTML/CSS (a fenced subset), the packer validates them at build time and produces engine-ready artifacts. Everything here is plain text — readable, editable, and predictable for both humans and coding agents.

## How to work here (for coding agents)

1. Run `loom list pkg` (CLI reference: `{{SKILLS_DIR}}/loom/SKILL.md`) to see what exists; `loom show <pkg>` for one package's pages and components. Do not bulk-scan the workspace.
2. Read the full fence reference at `{{SKILLS_DIR}}/loomgui-editor/SKILL.md` before writing or editing UI. It is the complete, authoritative rulebook (tags, roles, CSS whitelist, layout rules, build-error catalog).
3. Put HTML files under the package `dirs`. `<img src>` paths are relative to the HTML file itself (browser-native).
4. Validate with `loom check` and fix ALL reported errors in one pass — diagnostics are collect-all (file/line/column), not fail-fast.
5. Never edit files under the output directory. They are build artifacts and get overwritten on every build.

## Configuring the workspace (`loom` commands — never hand-edit `loom.workspace.json`)

- `loom new <name>` — create `ui/<name>/main.html` and register the package.
- `loom font add <file> --family <f> [--default] [--fallback]` — register a font (ask the user for the file if none exists yet).
- `loom atlas add <dir> [--name <n>]` — cover a PNG directory with an atlas.
- `loom list pkg|atlas|font` / `loom show <pkg>` — inspect current configuration.

## Building

- **CLI** (primary; bundled at `.loom/loom(.exe)`, no LoomGUI checkout needed): `loom check` → fix → `loom build`. Exit codes: 0 clean · 1 errors · 2 usage/config failure. `--format json` prints one machine-readable document to stdout.
- **GUI** (human-facing): open this workspace in the LoomGUI packer app and press the Build (打包) button. From Unity: menu `LoomGUI > Open Packer`.

Build outputs are written into `output_dir`, resolved against the Unity project recorded in `.loom/unity.json` when that file exists (typically straight into `Assets/Bundles`), otherwise local `dist/`:

- `ui/*.pkg.bin` — packaged UI, one file per package
- `atlas/{name}.png` + `{name}.atlas.json` — packed texture atlases with sprite UV maps
- `fonts/*.bytes` — font binaries
- `loom.runtime.json` — runtime bootstrap manifest

A failing build lists every fence violation and asset error together. Common asset errors: `<img src>` pointing at an image not covered by any atlas (missing-image), the same image covered by two atlases (atlas conflict), images not fitting `max_size` (atlas overflow), missing font file.

## Runtime integration (how Unity consumes this workspace)

Build artifacts are not the end of the chain — the game project loads and drives them:

- `LoomStageDriver` (a Unity component) reads `loom.runtime.json` at startup and loads everything it lists: packages, atlases (pages load lazily), fonts. In the editor it reads `Assets/Bundles` by default; in player builds the game serves the same files from StreamingAssets or its own AssetBundle/Addressables pipeline (virtual loading hooks on the driver).
- Game code shows a page with `driver.Instantiate("<package>", "<page-stem>")` (page stem = HTML filename without `.html`) and wires behavior on typed nodes, e.g. `page.Get<Button>("btn-start").Clicked += ...`.
- **Ids are the game-code API.** The `id` attributes you write in HTML are exactly what C# code looks up (`Get<T>("id")`). Keep ids of interactive elements stable and semantic (`btn-start`, `hp-fill`); renaming an id silently breaks game code.
- UI never blocks input by itself — the game gates its own 3D raycasts on `driver.Context.IsPointerOnUI`. Embedding 3D content inside UI (character previews, particles on cards) uses `driver.BindNativeHost`.
- Full Unity-side guide: `runtime-integration.md` under `Editor/Resources/LoomGUI/skill/references/` in the installed LoomGUI package.

## Workspace structure

```
workspace/
  loom.workspace.json       -> build configuration (see below)
  ui/                       -> HTML/CSS components (one .pkg.bin per package)
    showcase/
      main.html
      main.css              -> optional external CSS (via <link rel="stylesheet">)
  assets/                   -> image sources for atlases (PNG only)
    icons/
      home.png
  fonts/                    -> font files (.ttf / .otf / .ttc)
    NotoSansSC.ttc
  dist/                     -> build output (generated, never edit)
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
| `html` | string[] | Empty `[]` = auto-scan (packer scans `dirs` top-level `.html` files); non-empty = build only these listed files. Set back to `[]` to restore auto-scan. |

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

## Path conventions

All references resolve lexically at build time, browser-style:

- `<img src="...">` — relative to the **HTML file itself** (page or component file; components live in `components/`, so reaching outer assets takes extra `../`).
- `<link rel="stylesheet" href="...">` — relative to the HTML file. External CSS is fully supported: rules, `@keyframes`, and diagnostics behave exactly like inline `<style>`. A missing file is a build error (never a silent drop).
- `url()` inside a CSS file — relative to **that CSS file** (browser semantics; the packer rewrites it to the HTML-relative equivalent internally).

The packer converts image references to **sprite keys** (workspace-relative paths, forward slashes, globally unique). Images under `assets/` atlas directories are referenced by their workspace-relative path: `<img src="../../assets/icons/home.png">` -> sprite key `assets/icons/home.png`.

## Fence quick reference

The complete rulebook is the loomgui-editor skill (see "How to work here"). The essentials:

**Principle.** Tags plus `role` decide stable object types; CSS only grants behavior (`display:flex` switches the layout strategy, `overflow:auto` switches scroll). Nothing outside the fence is silently ignored — everything is a build error, reported together.

**Tags.** 8 document-shell tags are consumed at build time: `html`, `head`, `body`, `title`, `meta`, `style`, `link`, `script`. `link` carries real semantics for `rel="stylesheet"` (external CSS, see path conventions). 6 runtime tags enter the object tree: `div`, `span`, `button`, `img`, `template`, `slot`. Tag names containing a hyphen are custom elements; each must have a `components/<tag>.html` registration file in the package directory, otherwise the build fails with `UnregisteredCustomElement`. There is no `<br>` — split multi-line copy into separate block elements.

**Custom components are isolated (Shadow-DOM-like).** The CSS universe of a component instance is the component file's own `<style>`/`<link>` — page rules never reach inside, component rules never leak out. Projected children (what you write inside `<my-widget>`) are styled by the component's CSS; style them in the component file or carry sizes as content attributes. Components are validated standalone: a control inside a component must be matched by the component's own CSS. To share design tokens across pages and components, reference the same external CSS file from both sides.

**Controls and lists have no tags — they are role-driven.** Write them on a
`div` (role values are whitelist-checked; an unrecognized role is a build
error):

| role | Type | Required children (build-checked) |
|---|---|---|
| `combobox` | Dropdown | `role=listbox` child (containing `role=option`) |
| `listbox` | Container | at least one `role=option` child |
| `option` | OptionItem | — |
| `slider` | Slider | `data-slot=thumb` child |
| `spinbutton` | NumberField | — |
| `switch` | Toggle | — |
| `radio` | RadioButton | — |
| `progressbar` | ProgressBar | `data-slot=fill` child |
| `textbox` | TextField / TextArea with `aria-multiline=true` | — |
| `list` | ListView | `role=listitem` child (or a `template` whose first element child is `role=listitem`) |
| `listitem` | ListItem | — |
| `tablist` | TabList | `role=tab` child (panels link via `aria-controls`) |
| `tab` | Tab | — |
| `tabpanel` | plain Container | — (a panel is a div a tab points at via `aria-controls`) |
| `dialog` | plain Container | — (a modal overlay layer; standard WAI-ARIA vocabulary) |

Control initial values go into ARIA attributes (`aria-valuenow`, `aria-checked`, ...) or `data-*` (`data-step`, `data-name`). Plain value attributes on control divs are build errors — `value` is legal only on `role=option`.

**Controls ship with no default styles.** A control matched by no `<style>` rule is a build error (it would render blank) — style the control itself and its `data-slot` children; a `combobox` additionally requires `position:relative` on itself and `position:absolute` on its `role=listbox` popup. `switch` / `radio` have **no framework slot** (no knob slot — the framework only toggles `aria-checked`): visuals live in CSS state selectors, e.g. `[role="switch"][aria-checked="true"] .knob { transform: translateX(20px) }`.

**Layout rules that bite most often:**

- `display` accepts `block` / `flex` / `none` / `inline`. **`display:grid` does not exist** — build error.
- `button` and `img` are inline boxes: they must sit in a `display:flex` parent, or carry an explicit `display:block`. Bare in a block container is a build error (LoomGUI has no CSS inline flow outside flex).
- A block container's direct children must not mix inline-level (text, `span`, `img`) with block-level elements — wrap the inline run in a sub-`div`, or make the container `display:flex`. `slot` counts as block-level in this check (its projected content is unknowable at build time): put fallback text into the projected content, or give the slot its own container. Decorated frames (absolute-positioned background `img` + foreground content) hit this constantly — write them as `display:flex` + centering on the relative frame.
- `position` accepts only `absolute` / `relative`.
- `z-index` reorders siblings for drawing and hit-testing only (whole subtrees move with their parent); it never affects flex order — that is `order`.
- `transform` pivots around the element's center only (`transform-origin` does not exist); pivot around a non-center point by positioning at the arc midpoint.
- Animations: `animation` shorthand, `@keyframes`, and `:nth-child(An+B)` selectors are supported. Do not use `:nth-child` on virtualized lists (parked slots skew the count) — use `[data-index]` attribute selectors.

**Browser-difference traps** (the HTML preview lies): `background-image` without `background-size` stretches to fill in LoomGUI (browsers show original size); `border-width` without `border-style` draws a border in LoomGUI (browsers draw nothing); adjacent margins never collapse (prefer `gap` for spacing). `box-sizing`, `cursor`, `text-decoration`, `font-style`, and `text-transform` do not exist in the fence at all.
