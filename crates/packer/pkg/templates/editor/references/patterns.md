# Authoring patterns

Canonical, build-clean structures for the most common UI shapes. Adapt
colors and sizes; keep the structure.

## Control CSS (the required minimum)

A control matched by no `<style>` rule is a build error — it would render
blank. Canonical patterns:

```css
/* progressbar */
[role="progressbar"] { position: relative; width: 200px; height: 8px; background: #2e2e42; }
[role="progressbar"] [data-slot="fill"] { height: 100%; background: #7c5cfc; }

/* slider — thumb placement is OWNED by the control: horizontal offset follows
   the value, vertical is auto-centered on the track. Anchor it with
   left:0; top:0 and style size/look only; author top/left/margin positioning
   on the thumb is zeroed at runtime and flagged by check. */
[role="slider"] { position: relative; width: 200px; height: 4px; background: #2e2e42; }
[role="slider"] [data-slot="fill"] { position: absolute; left: 0; top: 0; height: 100%; background: #5fb4d4; }
[role="slider"] [data-slot="thumb"] { position: absolute; left: 0; top: 0; width: 16px; height: 16px; background: #fff; border-radius: 8px; }

/* combobox — structure contract: the selected-value anchor
   (data-slot="value") + the popup (role="listbox") MUST both be
   declared, otherwise FenceMissingControlChild fails the build. The
   runtime writes the selected option's text into the value slot's
   inner text node. */
[role="combobox"] { position: relative; display: flex; flex-direction: row; }
[role="combobox"] [data-slot="value"] { flex-grow: 1; }
[role="combobox"] [role="listbox"] { display: none; position: absolute; left: 0; top: 100%; width: 100%; }
[role="combobox"][aria-expanded="true"] [role="listbox"] { display: flex; flex-direction: column; }
```

`switch` / `radio` have **no framework slot** (no `data-slot="knob"` —
the framework never drives child geometry for these controls; it only
toggles the `aria-checked` state). Visual state lives entirely in CSS via
state selectors. A sliding knob is a child you author plus a
state-selector transform:

```css
[role="switch"] { position: relative; display: flex; width: 40px; height: 20px; background: #2e2e42; }
[role="switch"] .knob { position: absolute; left: 2px; top: 2px; width: 16px; height: 16px; background: #fff; transition: transform .15s; }
[role="switch"][aria-checked="true"] .knob { transform: translateX(20px); }
```

`tree` has no framework slot either — state lives in CSS via the
synthesized `aria-selected` / `aria-expanded` / `aria-level` attributes
(top item = 1; use it for indentation). Put every label (branch and
leaf) in a child `.row` element: a branch hosts its nested items as
block children, so a bare text label trips `FenceMixedInlineBlock`, and
a bare label misses the hover/selected/indent hooks anyway. Hiding
collapsed children is the runtime's job, never author `display:none`:

```css
[role="treeitem"] { display: flex; flex-direction: column; }
[role="treeitem"] .row { display: flex; flex-direction: row; align-items: center; gap: 8px; padding: 8px 12px; }
[role="treeitem"][aria-selected="true"] .row { background: #2e2e42; color: #7c5cfc; }
[role="treeitem"][aria-expanded="true"] .chev { transform: rotate(90deg); }
[role="treeitem"][aria-level="2"] .row { padding-left: 32px; }
```

## Decorated frames (background image behind foreground content)

Rings, campfire circles, portrait frames — the most common trigger of the
mixing rule. The frame is a flex container that centers; the background
image and the foreground content are both flex items:

```html
<div class="actor-frame">
  <img src="../../assets/frames/actor.png" alt="">
  <div class="actor-body">...</div>
</div>
```

```css
.actor-frame { position: relative; display: flex; align-items: center; justify-content: center; }
.actor-frame > img { position: absolute; }   /* stretch behind */
```

## Viewport panning (scroll containers, not Drag events)

Big content in a small viewport — maps, long feeds, panels — is the
`overflow:auto` scroll container. The gesture suite ships with it:
press-drag panning, wheel, inertia, boundary clamping, and
click-after-scroll cancellation. Write **zero drag math**:

```html
<div class="map-scroll">       <!-- overflow:auto — the viewport -->
  <div class="map-layer">      <!-- explicit content size, absolutely
                                  positioned children --> ... </div>
</div>
```

`.map-scroll` typically lives in an elastic flex panel
(`flex-grow:1; min-height:0`) — both are honored at runtime. The
low-level Drag events API is for **object dragging** (title bars,
drag-to-slot), not viewport panning; reaching for it here means
re-implementing what the scroll container already provides.

## Data-driven lists (virtualized)

Declare a blueprint; game code sets `ItemCount` and binds data at runtime:

```html
<div role="list" id="bag-items">
  <template>
    <div role="listitem" class="row">
      <img data-slot="icon" src="../assets/icons/slot.png" alt="">
      <div class="col">
        <span class="name">placeholder</span>
        <span class="count">x0</span>
      </div>
    </div>
  </template>
</div>
```

The list virtualizes automatically — slot count stays constant regardless
of `ItemCount`. Style rows via class selectors scoped under the list; do
not use `:nth-child` here (parked slots skew it), use `[data-index]`.

## Staggered entrance

```css
.card { animation: rise .4s both; }
.card:nth-child(3n+1) { animation-delay: .00s; }
.card:nth-child(3n+2) { animation-delay: .08s; }
.card:nth-child(3n+3) { animation-delay: .16s; }
@keyframes rise { from { opacity: 0; transform: translateY(24px); } }
```

Note: text nodes count as children — if delays come out wrong, count
every child.

## Shared design tokens across pages and components

Component style walls mean page CSS cannot style component internals. To
share tokens, author one external CSS and reference it from both sides
(each scope applies it independently):

```html
<!-- page: ui/game/main.html -->
<link rel="stylesheet" href="../../shared/tokens.css">
<!-- component: ui/game/components/actor-card.html -->
<link rel="stylesheet" href="../../../shared/tokens.css">
```
