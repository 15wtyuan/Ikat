# Fence schema reference

The complete tag, attribute, and role registry. The build enforces all of
it; nothing degrades silently.

## Document shell tags

`html`, `head`, `body`, `title`, `meta`, `style`, `link`, `script` —
consumed at build time, never present in the runtime tree. `link` carries
real semantics for `rel="stylesheet"` (external CSS); other rel values are
consumed without effect.

## Runtime tags

The complete list — `div`, `span`, `button`, `img`, `a`, `template`, `slot`:

| Tag | Type | Default display | Notes |
|---|---|---|---|
| `div` | Container | block | the universal box; controls and list items are divs with `role` |
| `span` | TextElement | inline | inline run inside rich-text blocks |
| `button` | Button | inline | content attrs: `disabled` (boolean, presence = true; maps to runtime disabled — clicks suppressed, `:disabled` pseudo matches, no hover hand); UA-defaults center its content |
| `img` | Image | inline (void) | content attrs: `src`, `alt`, `width`, `height` |
| `a` | Link | inline | content attr: `href` (opaque target id, must be non-empty); UA-default link color + underline (`:hover` recolors) unless overridden by author CSS; children restricted to text and non-flex `span`s — nesting links or images inside `a` is a build error |
| `template` | Template | none | inert blueprint holder (e.g. list item blueprints) |
| `slot` | Slot | inline | legal only inside component templates; attr `name` |

## Shell tags (document skeleton, consumed at pack time — no runtime nodes)

| Shell tag | Purpose |
|---|---|
| `html` | document root, consumed by the parser |
| `head` | metadata container |
| `body` | content root — its children become the template root |
| `title` | document title |
| `meta` | metadata |
| `style` | inline CSS (consumed at pack time) |
| `link` | external stylesheet reference (`rel=stylesheet`; missing file is a build error) |
| `script` | outside the fence — build error or skipped |

Any other tag is a build error (`FenceUnknownTag`). There is no `p`,
`header`, `input`, `select`, `ul`, `label`, and no `<br>` — split
multi-line copy into separate block elements (every line break is
structure).

## Global attributes

Every element accepts: `id`, `class`, `style`, `slot`, `hidden`,
`tabindex`, `type`, `role`, `draggable`, `part`, plus any `aria-*`,
`data-*`, and `--*`-prefixed attributes. `id` must be unique within a template
scope; `aria-controls` / `aria-labelledby` must point at existing ids.
A `--*` attribute is passthrough data (matchable by `[attr]` selectors)
— the var() sources are `<style>` rules, inline `style`, and runtime
`Style.SetVar` (see css-reference.md "Custom properties and var()").

`part="name"` marks a node inside a component template as a styling
hook for the page's `::part()` selector (see css-reference.md
"Selectors"). It only carries `::part()` semantics inside an expanded
component; a page-level `part` attribute is writable but matches
nothing (no shadow boundary to pierce), and `[part="x"]` is **not**
equivalent to `::part(x)`.

`draggable="true"` opts the element into the drag event chain
(`DragStart` / `DragMove` / `DragEnd` fire after pointer-down passes the
movement threshold). Only `true` and `false` are accepted — the browser
enum value `auto` is a build error (there is no native drag to inherit
from in a self-drawn engine); default is `false`.

## Custom elements

A tag name containing `-` (e.g. `<my-widget>`) is a CustomElement. It must
be registered as `components/<tag>.html` inside the package directory —
that file IS the registration; the packer expands instances with slot
projection. Unregistered = build error. A `<slot>` at page level (outside
a component template) is also a build error.

## Component isolation (Shadow-DOM-like, one model with three faces)

- **Style wall**: the CSS universe of a component instance is the
  component file's own `<style>` / `<link>` — page rules never reach
  inside a component, component rules never leak out. To share visuals
  (design tokens, common control styles), reference the same external CSS
  file from both the page and the component — same file, each scope
  applies it independently.
- **Projected children belong to the component scope**: the elements you
  put inside `<my-widget>` are styled by the component's CSS, not the
  page's. Give them content attributes (e.g. `width`/`height` on `img`)
  or style them inside the component file.
- **Standalone validation**: a component file is validated on its own —
  a control inside a component must be matched by the component's own
  CSS (`FenceControlWithoutCss` otherwise); page CSS cannot save it.

## Role registry

Controls and lists have no dedicated tags — write them on a `div` with a
WAI-ARIA `role`. Role values are whitelist-checked (`FenceUnknownRole`
on typos). Required children must be **direct** children (wrapping them
in an intermediate div fails the check); the only exception is the
`list` + `template > role=listitem` blueprint for data-driven lists.

| role | Type | Required direct children (build-checked) |
|---|---|---|
| `combobox` | Dropdown | `role=listbox` child (which itself needs `role=option` children) **and** a `data-slot=value` child (the selected-value display) |
| `listbox` | Container | at least one `role=option` child |
| `option` | OptionItem | none; may carry `value` (the only place `value` is legal) |
| `slider` | Slider | `data-slot=thumb` child |
| `spinbutton` | NumberField | none |
| `switch` | Toggle | none |
| `radio` | RadioButton | none |
| `progressbar` | ProgressBar | `data-slot=fill` child |
| `textbox` | TextField, or TextArea with `aria-multiline="true"` | none |
| `list` | ListView | `role=listitem` child, or a `template` whose first element child is `role=listitem` |
| `listitem` | ListItem | none |
| `tablist` | TabList | `role=tab` children; optional `data-activation="manual"` (values `manual` / `automatic`, default `automatic`, anything else is a build error). `manual` = arrow keys move focus only, Enter/Space commits the selection; `automatic` = arrows select immediately with focus following |
| `tab` | Tab | none (may also be written `button role=tab`) |
| `tree` | Tree | `role=treeitem` children at any nesting depth (items nest directly inside treeitem — no `role=group` wrapper layer). Single-select tree (focus move selects, APG single-select model); keyboard = APG Tree View core set: Up/Down move between visible items, Right expands / enters first child, Left collapses / goes to parent, Home/End first/last visible item, Enter/Space activates (select + branch toggle). Typeahead (APG optional) is not supported |
| `treeitem` | TreeItem | direct `role=treeitem` children make it a branch (expand/collapse state; `aria-expanded="true"` bakes the initial state, default collapsed); no nested items = leaf. Selection derives from the owning Tree (`aria-selected` is synthesized); initial selection = first item with `aria-selected="true"`, else the first item. A branch label goes in a child element (wrapper div) — a branch hosts its nested items as block children, so a bare text label trips `FenceMixedInlineBlock`; leaves with plain text are fine. Style hooks: `[aria-selected="true"]`, `[aria-expanded="true"]`, `[aria-level="N"]` (level derives from nesting depth, top level = 1 — use for indentation) |
| `tabpanel` | plain Container | none — a panel is a div a tab points at via `aria-controls`; hiding inactive panels is the TabList runtime's job, never author `display:none` (it bakes into the packed base style and keeps the active panel invisible — `FenceTabpanelHiddenByAuthor`) |
| `dialog` | plain Container | none — a modal overlay layer |

Initial values go into ARIA (`aria-valuenow`, `aria-valuemin`,
`aria-valuemax`, `aria-checked`,
`aria-selected`, ...) or `data-*` (`data-step`, `data-name`) — never plain
attributes (`value` is legal only on `role=option`). Progressbar fill
follows ARIA math: `(valuenow - valuemin) / (valuemax - valuemin)`
(`valuemin` defaults to 0, so `valuenow/valuemax` when absent).

Canonical CSS for each control shape: `patterns.md`.
