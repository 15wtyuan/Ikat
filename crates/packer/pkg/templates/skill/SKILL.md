---
name: loomgui-editor
description: |
  Author LoomGUI game UI in a fenced standard HTML/CSS subset. Covers the
  tag + role schema, flex-only inline layout rules, control structure and
  CSS contracts, the CSS property whitelist, and the build-error
  self-correction loop. Use for any task in a LoomGUI workspace: creating
  or editing screens, controls, lists, styling, animations, or fixing
  packer build errors.
---

# LoomGUI Editor

Author and self-correct fence-compliant UI for LoomGUI. Everything below is
the complete rulebook; when the build disagrees with this file, trust the
build error message.

## Core principle

**Tags (plus `role`) decide stable object types; CSS grants behavior but
never changes types.** `<button>` is always a Button, `<div role="slider">`
is always a Slider. `display:flex` switches the layout strategy,
`overflow:auto` switches scroll — no rebuild, no state loss. Anything
outside the fence is a **build-time error**, never a silent ignore, and all
diagnostics are reported together (collect-all, with file/line/column).

## Workflow

1. **Read `loom.workspace.json`** at the workspace root. It defines
   packages (`dirs` to place HTML in), atlases (image directories), and
   fonts.
2. **Write HTML + CSS** using the schema below. Place files under the
   package `dirs`. `<img src>` is relative to the HTML file. New images not
   referenced by any `<img>` must be covered by an atlas `dirs` entry.
3. **Build and self-correct.** Build entry points:
   - GUI: open the workspace in the LoomGUI packer app, press Build (打包).
   - CLI (only inside a LoomGUI repository checkout):
     `cargo run -p loomgui_pkg -- build <workspace-root>`.
   A failing build lists **all** violations at once. Fix every diagnostic in
   one pass, then rebuild. Zero errors = artifacts are in `output_dir`
   (`ui/*.pkg.bin`, `atlas/*`, `fonts/*.bytes`, `loom.runtime.json`).
4. **Report** artifact paths. Never edit files under the output directory —
   they are regenerated.

## Tag schema

**Document shell (consumed at build time, never in the runtime tree):**
`html`, `head`, `body`, `title`, `meta`, `style`, `link`, `script`.

**Runtime tags (the complete list — `div`, `span`, `button`, `img`, `template`, `slot`):**

| Tag | Type | Default display | Notes |
|---|---|---|---|
| `div` | Container | block | the universal box; controls/list items are divs with `role` |
| `span` | TextElement | inline | inline run inside rich-text blocks |
| `button` | Button | inline | content attrs: `disabled`; UA-defaults center its content |
| `img` | Image | inline (void) | content attrs: `src`, `alt`, `width`, `height` |
| `template` | Template | none | inert blueprint holder (e.g. list item blueprints) |
| `slot` | Slot | inline | legal only inside component templates; attr `name` |

Any other tag is a build error (`FenceUnknownTag`) — there is no `p`,
`header`, `input`, `select`, `ul`, `label`, etc. Use `div` with CSS and, for
controls, `role`.

**Custom elements**: a tag name containing `-` (e.g. `<my-widget>`) is a
CustomElement. It must be registered as `components/<tag>.html` in the
package directory (this file IS the registration; the packer expands
instances with slot projection). Unregistered = build error. A `<slot>` at
page level (outside a component template) is also a build error.

**Global attributes** (every element): `id`, `class`, `style`, `slot`,
`hidden`, `tabindex`, `type`, `role`, plus any `aria-*`, `data-*`, and CSS
custom property `--*`. `id` must be unique within a template scope.
`aria-controls` / `aria-labelledby` must point at existing ids.

## Role-driven controls and lists

Controls and lists have no dedicated tags. Write them on a `div` with a
WAI-ARIA `role`. Role values are whitelist-checked: an unrecognized value
(often a typo, e.g. `role="silder"`) is a build error (`FenceUnknownRole`) —
otherwise the element would silently degrade to a plain container and skip
every control validation.

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

Required children must be **direct** children (wrapping them in an
intermediate div fails the check); the only exception is the `list` +
`template > role=listitem` blueprint for data-driven lists.

**Initial values** go into ARIA (`aria-valuenow`, `aria-checked`,
`aria-selected`, ...) or `data-*` (`data-step`, `data-name`) — never plain
attributes (`value` is legal only on `role=option`).

**Controls have NO framework styles.** A control element matched by no
`<style>` rule is a build error (`FenceControlWithoutCss`) — without CSS it
would render blank. Style the control itself and its `data-slot` children.
Canonical patterns (adapt colors/sizes):

```css
/* progressbar */
[role="progressbar"] { position: relative; width: 200px; height: 8px; background: #2e2e42; }
[role="progressbar"] [data-slot="fill"] { height: 100%; background: #7c5cfc; }

/* slider */
[role="slider"] { position: relative; width: 200px; height: 4px; background: #2e2e42; }
[role="slider"] [data-slot="thumb"] { position: absolute; left: 50%; width: 16px; height: 16px; background: #fff; }

/* combobox — structure contract: the anchor + the popup MUST both be declared,
   otherwise FenceControlStructureCss fails the build */
[role="combobox"] { position: relative; }
[role="combobox"] [role="listbox"] { display: none; position: absolute; left: 0; top: 100%; width: 100%; }
[role="combobox"][aria-expanded="true"] [role="listbox"] { display: flex; flex-direction: column; }
```

For `switch` / `radio`, an attribute selector keyed on state reads best:
`[role="switch"][aria-checked="true"] { ... }`.

## Layout rules

- `display: block | flex | none | inline`. **`display:grid` does not exist**
  (build error). Flex defaults to `flex-direction: row` (standard CSS).
- **Inline boxes (`button`, `img`) must live in a `display:flex` parent**,
  or carry an explicit `display:block`. Bare in a block container is a
  build error: LoomGUI has no CSS inline flow outside flex, so the element
  would stack full-width and break browser-trained expectations. `span` and
  `slot` are exempt (they join rich-text runs).
- **No mixing inside one block container**: the direct children of a
  `display:block` container must be all inline-level (text / `span` /
  `img` — that combination becomes a rich-text block, laid out like browser
  inline flow) or all block-level. Mixed = build error. Fix by wrapping the
  inline run in a sub-`div`, or switching the container to `display:flex`.
- `position: absolute | relative` only (`fixed`/`sticky` are build errors).
- `z-index` reorders **siblings** for drawing and hit-testing; a whole
  subtree moves with its parent; there are no nested stacking contexts. It
  never changes flex order (that is `order`).
- Spacing: use `gap` — margins are supported but never collapse, so
  `margin` stacking surprises differ from browsers.
- Scrolling: `overflow: auto | scroll` on any box selects the scroll
  strategy.

## CSS reference

<!-- fence-sync:css-supported-begin -->
Supported properties (complete whitelist; everything else is a build error):

- `width` / `height` / `min-width` / `min-height` / `max-width` / `max-height` — px, %, auto
- `display` — block / flex / none / inline (grid rejected)
- `flex-direction` / `flex-wrap` / `flex-grow` / `flex-shrink` / `flex-basis`
- `gap` / `row-gap` / `column-gap`
- `justify-content` / `align-items` / `align-content` / `align-self`
- `order` / `aspect-ratio` / `z-index` — integer, no `auto`
- `position` — absolute / relative; with `top` / `right` / `bottom` / `left`
- `padding-top` / `padding-right` / `padding-bottom` / `padding-left`
- `margin-top` / `margin-right` / `margin-bottom` / `margin-left`
- `border-color` / `border-style` / `border-radius` / `border-image-slice`
- `background-color` / `background-image` / `background-size` / `background-repeat` / `background-clip` / `-webkit-background-clip`
- `opacity` / `box-shadow` / `pointer-events` / `transform` / `filter`
- `color` / `font-size` / `font-family` / `font-weight`
- `text-align` / `line-height` / `letter-spacing` / `white-space` / `text-shadow`
- `-webkit-text-stroke` / `font-effect` — LoomGUI text extensions
- `caret-color` / `selection-background` / `selection-color` / `placeholder-color` / `-webkit-text-security` — text-control theming
- `animation` and longhands: `animation-name` / `animation-duration` / `animation-timing-function` / `animation-delay` / `animation-iteration-count` / `animation-direction` / `animation-fill-mode` / `animation-play-state`
- `transition`
- `overflow-x` / `overflow-y` — visible / hidden / scroll / auto
- `resize` — accepted as a no-op (never consumed)

Shorthands (expand to the properties above):

- `padding` — four-side box
- `margin` — four-side box
- `overflow` — sets both axes
- `border` — color-led border shorthand
- `border-width` — four-side box
- `border-top` — single side
- `border-right` — single side
- `border-bottom` — single side
- `border-left` — single side
- `background` — color, image, size, repeat
- `flex` — grow, shrink, basis
<!-- fence-sync:css-supported-end -->

`background-image` accepts `none`, `url(...)`, and a gradient subset:
`linear-gradient` and `radial-gradient` (up to 8 stops, hex/rgb()/rgba()
colors). `background-size` accepts `cover` / `contain` / `100%` /
`stretch`. `filter` accepts grayscale / brightness / contrast / saturate /
hue-rotate / invert / sepia. `transform` accepts translate / rotate /
scale.

**Value rejections to remember** (property exists, value does not):
`display:grid`, `flex-wrap:wrap-reverse`, `position:fixed`,
`position:sticky`, `z-index:auto`, `conic-gradient`,
`repeating-linear-gradient`, `repeating-radial-gradient`, and the combinator
selectors `>` / `+` / `~`.

<!-- fence-sync:css-not-supported-begin -->
Properties that do NOT exist in the fence (using any of these is a
`FenceUnknownCssProp` build error):

- `box-sizing` — there is no border-box switch; padding adds to the set width/height
- `cursor`
- `text-decoration`
- `font-style` — no italic via CSS (and no `em` / `i` tags either)
- `text-transform`
- `user-select` — use `pointer-events` for interaction gating
- `vertical-align`
- `float`
- `background-position`
- `object-fit`
- `word-break`
- `text-overflow`
- `list-style`
- `clip-path`
<!-- fence-sync:css-not-supported-end -->

**Animations.** Define with `@keyframes <name> { from {...} to {...} 50% {...} }`
inside `<style>`, apply via the `animation` shorthand
(`<name> <duration> [easing] [count|infinite] [fill-mode] [direction] [delay]`,
e.g. `animation: fadeIn .4s .05s both`). `:nth-child(An+B | odd | even | N)`
selectors work in `<style>` rules — handy for staggered entrances. Do not
use `:nth-child` on virtualized lists (`role=list` bound to data): parked
slots count as children and skew the index; use `[data-index="N"]`
attribute selectors instead.

**Browser-difference warnings** (the build flags these, previews mislead):

- `background-image` without `background-size`: browsers show the original
  size, LoomGUI stretches to fill.
- `border-width` without `border-style`: browsers draw nothing, LoomGUI
  draws the border.
- Adjacent margins never collapse (browsers collapse them vertically).
- No inline flow outside flex containers and rich-text blocks.

## Build errors (complete catalog)

| Code | Meaning |
|---|---|
| `FenceUnknownTag` | tag not in the fence (and not a hyphenated custom element) |
| `FenceUnknownAttr` | attribute not allowed on the element |
| `FenceUnknownCssProp` | CSS property outside the whitelist |
| `FenceBadCssValue` | CSS value rejected (bad keyword, bad gradient, bad length, ...) |
| `FenceBadAttrValue` | structural attribute value outside its domain |
| `DuplicateId` | duplicate `id` in one template scope |
| `UnclosedTag` | tag not closed |
| `InvalidContentModel` | child not allowed by the parent's content model |
| `UnregisteredCustomElement` | hyphenated tag with no `components/<tag>.html` |
| `InvalidAriaRelation` | `aria-controls` / `aria-labelledby` target missing |
| `TokenizerError` | unrecoverable HTML lexer error |
| `FenceInlineElementInBlockContext` | `button`/`img` bare in a block container |
| `FenceMixedInlineBlock` | block container mixing inline-level and block-level direct children |
| `FenceBorderWithoutStyle` | warning: `border-width` without `border-style` |
| `FenceBgImageWithoutSize` | warning: `background-image` without `background-size` |
| `FenceControlWithoutCss` | control element matched by no `<style>` rule |
| `FenceControlStructureCss` | control structure CSS missing (combobox anchor / popup positioning) |
| `FenceMissingControlChild` | control missing a required child role or `data-slot` |
| `FenceUnknownRole` | `role` value not in the role registry (typo guard — copy role names from the table above verbatim) |

## Preview trust

Trust: flex layout, `gap`, px units, colors, gradients subset,
`position:absolute`, `border-radius`, `@keyframes` timing.
Distrust: inline flow outside flex, margin collapsing, default
`background-size`, `border-style`-less borders, anything this file says the
fence rejects. Rule of thumb: **trust the build output, not the browser
preview.**
