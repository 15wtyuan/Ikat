---
name: ikat-editor
description: |
  Author and self-correct UI screens for Ikat — the fenced standard
  HTML/CSS subset that compiles into the game's runtime UI. Use for ANY
  task touching HTML or CSS in a Ikat workspace (the directory tree
  holding ikat.workspace.json, located via .ikat/config.json at the
  session root): creating or editing screens, components, controls,
  lists, styling, animations, or diagnosing packer build errors.
---

# Ikat UI Authoring

Write fence-compliant UI and converge to a clean build. Rendering parity
between the browser preview and the game runtime is a framework guarantee,
enforced at build time: anything that would render differently is either a
build error or a build warning. When the build disagrees with this skill,
trust the build message.

## Boundaries

- Editing C# game code that drives pages/nodes/events → `ikat-runtime`.
- Preview simulation scripts and the `ikat preview` loop → `ikat-preview`.
- Workspace configuration, build invocation, CLI flags → `ikat`.
- HTML/CSS authoring and fence diagnostics → this skill.

## Prerequisites

- Session root has `.ikat/` (config + `ikat` CLI). Read `.ikat/config.json`
  first: `ui_root` is where sources live, `unity_root` (if present) is where
  `ikat build` delivers artifacts.
- Text needs a registered default font (`ikat font add <file> --family <f>
  --default`). No font yet → ask the user for a font file before writing
  screens full of text.
- Every image referenced by `<img>` must live under an atlas directory
  (`ikat atlas add <dir>`); the build cross-validates coverage.

## Critical rules

1. **Tags plus `role` decide stable object types; CSS grants behavior but
   never changes types.** `<button>` is always a Button, `<div role=slider>`
   is always a Slider. `display:flex` switches layout strategy, `overflow:
   auto` switches scroll — no rebuild, no state loss.
2. Everything outside the fence is a **build-time error, never a silent
   ignore**. Diagnostics are collect-all (every error in every file, with
   file/line/column) — fix them ALL in one pass, then re-check.
3. The complete runtime tag set is `div`, `span`, `button`, `img`, `a`, `template`, `slot`
   (+ hyphenated custom elements). There is no `p`,
   `header`, `input`, `select`, `ul`, `label`, and **no `<br>`** — every
   line break is structure: split multi-line copy into separate block
   elements. Shell tags (`html`, `head`, `body`, `title`, `meta`, `style`,
   `link`, `script`) are consumed at build time.
4. Controls and lists have no tags — write them on a `div` with a WAI-ARIA
   `role` (whitelist-checked; a typo like `role="silder"` is a build error).
5. **Controls have no framework styles.** A control matched by no `<style>`
   rule renders blank — that is a build error (`FenceControlWithoutCss`).
6. Inline boxes (`button`, `img`) must live in a `display:flex` parent or
   carry an explicit `display:block`. There is no CSS inline flow outside
   flex.
7. Direct children of a `display:block` container must not mix inline-level
   and block-level. `slot` counts as block-level (its projected content is
   unknowable at build time).
8. `display:grid` does not exist. Values: `block` / `flex` / `none` /
   `inline`.
8a. Pseudo-classes `:hover` / `:active` / `:focus` / `:disabled` /
    `:checked` / `:nth-child(...)` work in `<style>` rules and
    re-evaluate every frame — hover styling needs no runtime class
    toggling. `*`, `:not()`, pseudo-elements (`::before`), and
    combinators `>` `+` `~` are build errors; the diagnostic names the
    offending construct.
9. `position` accepts `absolute` / `relative` only.
10. Components are isolated (Shadow-DOM-like): a component's CSS universe
    is its own `<style>`/`<link>`; page rules never reach inside, component
    rules never leak out; each component is validated standalone.
    **Slot-projected content belongs to the component universe**: content
    the page projects into a component `<slot>` is styled by the
    component's own `<style>` (plain selectors — no `::slotted()`), and
    page rules targeting it are dead code (build warning). This is the
    opposite of flattened browser previews, which show page CSS winning —
    do not trust the preview on projected subtrees.
10a. Inline text elements (`span`) are folded into the parent's rich-text
    flow and have **no box of their own**: `width`/`height` on them never
    apply (same as browsers — the build warns). To make a sized dot/chip,
    use a flex item (`<div>` child of a `display:flex` row) or `<img>`;
    `display:inline-block` does not exist. A span that itself gets
    `display:flex` (class rule) switches to a flex container and its
    children become sizeable flex items.
11. Path bases are browser semantics: `<img src>` and `<link href>` are
    relative to the HTML file; `url()` inside a CSS file is relative to
    that CSS file. A missing file is a build error, never a silent drop.
12. `transform` pivots around the element's **center** only
    (`transform-origin` does not exist).
13. `z-index` reorders siblings for drawing and hit-testing only; it never
    affects flex order — that is `order`.
14. **Ids are the game-code API** (see `ikat-runtime`): keep interactive
    ids stable and semantic (`btn-start`, `hp-fill`). Renaming an id
    silently breaks C#. Ids must be unique per template scope;
    `aria-controls`/`aria-labelledby` must point at existing ids.
15. Initial values go into ARIA (`aria-valuenow`, `aria-checked`, ...) or
    `data-*`; a plain `value` attribute is legal only on `role=option`.
16. Never edit files under the output directory — they are regenerated.

Full lookup tables (tags, attributes, the 15-role registry, the CSS
whitelist, canonical control CSS) are in `references/` — load them on
demand, not upfront.

## Workflow

1. **Orient.** Read `.ikat/config.json` → `ui_root`. `ikat list pkg` /
   `ikat show <pkg>` to see what exists. Never bulk-scan the workspace.
2. **Author.** Write HTML/CSS under the package `dirs`. Check
   `references/fence-schema.md` for the tag/role tables and
   `references/patterns.md` for canonical control styling before
   inventing structure.
3. **Check and fix in rounds.** `ikat check` (from the session root, or
   `ikat check <ui-dir>`) → fix EVERY diagnostic in one editing pass →
   repeat until exit 0. Diagnostics carry code/file/line/column/help.
4. **Write preview simulation.** If the page has `data-fill` lists,
   custom elements, interactive controls, or game-code-driven content,
   write the preview scripts per the `ikat-preview` skill
   (`preview/main.js` + `preview/pages/<page>.js` — the server injects
   them; HTML stays clean). A `PreviewDataFillWithoutSim` warning means
   this step is missing.
5. **Human preview (the gate).** Start or reuse `ikat preview` (running
   instance reports the same URL; also recorded in
   `.ikat/preview.json`), give the human the URL, and iterate on their
   feedback — they refresh to see each fix. Self-check the same way if
   in doubt: the preview workbench renders at the design resolution
   under `match_mode` scaling and never reflows. Trust: flex layout,
   `gap`, px, colors, gradients subset, `position:absolute`,
   `border-radius`, `@keyframes` timing. Distrust: the
   browser-difference list in `references/css-reference.md` — the build
   already flags each of those with a warning (border without style,
   bg-image without size, non-transitionable `transition` properties,
   dead sizing on inline text, page rules over projected content); if
   there are no warnings, the preview is honest.
6. **Build on approval.** Only after the human approved the preview (or
   explicitly waived it): `ikat build` and report the artifact paths
   from the report.

## Pre-flight checklist (before reporting done)

- [ ] `ikat check` exits 0.
- [ ] Warnings handled, not just survived: `FenceBorderWithoutStyle` /
      `FenceBgImageWithoutSize` mean the preview will NOT match the runtime.
- [ ] No id of an interactive element was renamed (game code depends on it).
- [ ] New images are covered by an atlas dir; new fonts registered.
- [ ] Preview simulation written (step 4) — no `PreviewDataFillWithoutSim`.
- [ ] Human previewed the page via `ikat preview` and approved (or
      explicitly waived preview); artifacts rebuilt if sources changed.

## Failure patterns

- ❌ `<div><button>start</button></div>` — inline box bare in block context
  → ✅ make the container `display:flex`, or give the button
  `display:block`.
- ❌ Decorated frame: `position:relative` box + absolute background `<img>`
  + content `<div>` (trips the mixing rule constantly) → ✅ the frame is
  `display:flex; align-items:center; justify-content:center`, background
  image and content both flex items.
- ❌ One text node with embedded newlines expecting line breaks → ✅ one
  block element per line (there is no `<br>`).
- ❌ `:nth-child` stagger on a virtualized `role=list` (parked slots skew
  the count) → ✅ `[data-index="N"]` attribute selectors.
- ❌ Styling a component's internals from the page → ✅ style it inside the
  component file, or reference a shared external CSS file from both sides.

## Error recovery

| Code | Typical cause → fix |
|---|---|
| `FenceUnknownTag` | tag outside the fence → use the 6 runtime tags (+ custom elements) |
| `FenceUnknownRole` | role typo → copy role names from the registry verbatim |
| `FenceControlWithoutCss` | control matched by no rule → add a `[role=...]` or class rule styling it |
| `FenceInlineElementInBlockContext` | bare `button`/`img` in block → flex parent or `display:block` |
| `FenceMixedInlineBlock` | mixed children → wrap the inline run in a sub-`div` or switch to flex |
| `FenceStylesheetNotFound` | `<link>` href misses → fix the path (relative to the HTML file) |
| `SpriteMissingFromAtlas` | image not under any atlas dir → `ikat atlas add` |
| `DuplicateId` | same id twice in a template scope → rename |
| `UnregisteredCustomElement` | hyphenated tag without `components/<tag>.html` → create the file |

The build error text itself carries file/line/column and a help line — read
it before guessing.

## References

| File | Contents |
|---|---|
| `references/fence-schema.md` | tag tables, global attributes, custom elements, component isolation, the 15-role registry |
| `references/css-reference.md` | CSS whitelist, value domains, browser-difference traps, animations |
| `references/patterns.md` | canonical control CSS, decorated frames, list blueprints, staggered entrances |
