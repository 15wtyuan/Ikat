# CSS reference

Supported properties (complete whitelist; everything else is a build
error):

<!-- fence-sync:css-supported-begin -->
- `width` / `height` / `min-width` / `min-height` / `max-width` / `max-height` — px, %, auto
- `display` — block / flex / none / inline (grid rejected)
- `flex-direction` / `flex-wrap` / `flex-grow` / `flex-shrink` / `flex-basis`
- `gap` / `row-gap` / `column-gap`
- `justify-content` / `align-items` / `align-content` / `align-self`
- `order` / `aspect-ratio` / `z-index` — integer, no `auto`
- `position` — absolute / relative / static (initial value `static`); with `top` / `right` / `bottom` / `left` (px / % / auto; `%` resolves against the containing block — browser semantics)
- **Containing block of an `absolute` element = nearest ancestor with `position: relative` or `absolute`** (browser semantics); if none, the viewport. Known limits: an `absolute` element with all four insets `auto` keeps its direct-parent static position (browser hypothetical-box semantics not implemented); overflow clipping still follows DOM ancestors.
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

## Value domains

- `background-image`: `none`, `url(...)`, `linear-gradient` /
  `radial-gradient` (up to 8 stops, hex / `rgb()` / `rgba()` colors).
  `conic-gradient` and `repeating-*` variants do not exist.
- `background-size`: `cover` / `contain` / `100%` / `stretch`.
- `filter`: grayscale / brightness / contrast / saturate / hue-rotate /
  invert / sepia.
- `transform`: translate / rotate / scale. Rotations pivot around the
  element's **center** (`transform-origin` does not exist); to pivot
  around a non-center point, position the element at the midpoint of the
  desired arc and rotate.
- `transition`: animates `background-color` / `color` / `opacity` /
  `transform` (interpolated as a decomposed translate-scale-rotate).
  Everything else — layout properties (`width`, `margin`, ...),
  `box-shadow`, `filter` — changes instantly; the build warns per
  property.
- `position`: `absolute` / `relative` / `static` — `fixed` and `sticky` are build
  errors.
- `z-index`: integer only, no `auto`.

## Selectors

A selector is a chain of compounds separated by whitespace (descendant
combinator). Each compound is `tag? (.class | #id | [attr] | :pseudo)*`:

- Pseudo-classes that work: `:hover`, `:active`, `:focus`, `:disabled`,
  `:checked`, `:nth-child(An+B | odd | even | N)`. They gate on live
  interaction state and re-evaluate every frame — `:hover` driven
  styling needs no runtime class toggling.
- Build errors (the diagnostic names the offending construct):
  combinators `>` / `+` / `~` (descendant only), the universal selector
  `*`, unknown pseudo-classes (`:not()`, `:nth-of-type`, ...), and
  pseudo-elements (`::before`, `::after`, ...).
- Attribute selectors: `[attr]` and `[attr="value"]` only; higher
  operators (`^=`, `~=`, `$=`, `*=`, `|=`) are build errors.
- Do not use `:nth-child` on virtualized lists (`role=list` bound to
  data): parked slots count as children and skew the index — use
  `[data-index="N"]` instead.

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

## Animations

Define with `@keyframes <name> { from {...} to {...} 50% {...} }` inside
`<style>`, apply via the `animation` shorthand (`<name> <duration>
[easing] [count|infinite] [fill-mode] [direction] [delay]`, e.g.
`animation: fadeIn .4s .05s both`). Identical duplicate keyframes across
component instances merge silently; same name with different content
warns (host wins) — prefer defining shared animations page-level or in a
shared external CSS and referencing the name from components.

## Browser-difference traps (preview honesty)

The build flags every one of these with a warning; with zero warnings the
browser preview is honest.

- `background-image` without `background-size`: browsers show the
  original size, LoomGUI stretches to fill.
- `border-width` without `border-style`: browsers draw nothing, LoomGUI
  draws the border.
- Adjacent margins never collapse (browsers collapse them vertically);
  prefer `gap` for spacing.
- No inline flow outside flex containers and rich-text blocks.
