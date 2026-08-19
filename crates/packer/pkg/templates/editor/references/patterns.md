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

/* slider */
[role="slider"] { position: relative; width: 200px; height: 4px; background: #2e2e42; }
[role="slider"] [data-slot="thumb"] { position: absolute; left: 50%; width: 16px; height: 16px; background: #fff; }

/* combobox — structure contract: the anchor + the popup MUST both be
   declared, otherwise FenceControlStructureCss fails the build */
[role="combobox"] { position: relative; }
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
