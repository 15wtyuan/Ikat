# Fence schema reference

The complete tag, attribute, and role registry. The build enforces all of
it; nothing degrades silently.

## Document shell tags

`html`, `head`, `body`, `title`, `meta`, `style`, `link`, `script` —
consumed at build time, never present in the runtime tree. `link` carries
real semantics for `rel="stylesheet"` (external CSS); other rel values are
consumed without effect.

## Runtime tags

The complete list — `div`, `span`, `button`, `img`, `template`, `slot`:

| Tag | Type | Default display | Notes |
|---|---|---|---|
| `div` | Container | block | the universal box; controls and list items are divs with `role` |
| `span` | TextElement | inline | inline run inside rich-text blocks |
| `button` | Button | inline | content attrs: `disabled`; UA-defaults center its content |
| `img` | Image | inline (void) | content attrs: `src`, `alt`, `width`, `height` |
| `template` | Template | none | inert blueprint holder (e.g. list item blueprints) |
| `slot` | Slot | inline | legal only inside component templates; attr `name` |

Any other tag is a build error (`FenceUnknownTag`). There is no `p`,
`header`, `input`, `select`, `ul`, `label`, and no `<br>` — split
multi-line copy into separate block elements (every line break is
structure).

## Global attributes

Every element accepts: `id`, `class`, `style`, `slot`, `hidden`,
`tabindex`, `type`, `role`, plus any `aria-*`, `data-*`, and CSS custom
property `--*`. `id` must be unique within a template scope;
`aria-controls` / `aria-labelledby` must point at existing ids.

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
| `combobox` | Dropdown | `role=listbox` child, which itself needs `role=option` children |
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
| `tablist` | TabList | `role=tab` children |
| `tab` | Tab | none (may also be written `button role=tab`) |
| `tabpanel` | plain Container | none — a panel is a div a tab points at via `aria-controls` |
| `dialog` | plain Container | none — a modal overlay layer |

Initial values go into ARIA (`aria-valuenow`, `aria-checked`,
`aria-selected`, ...) or `data-*` (`data-step`, `data-name`) — never plain
attributes (`value` is legal only on `role=option`).

Canonical CSS for each control shape: `patterns.md`.
