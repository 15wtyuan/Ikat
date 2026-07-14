# Showcase browser preview

Double-click `../home.html` to preview in browser (visual reference, not behavioral mirror).

## Trustworthy (aligns with runtime)
flex layout/direction/gap/justify/align; px/% sizing; color/opacity/border/radius;
background-image/size; filter; transform; overflow:scroll; border-image-slice 9-grid; list skeleton.

## Approximate (layout drift, not pixel-perfect)
- text wrapping/spacing: Chrome vs unicode-linebreak, break points may differ.
- tween animation: CSS transition approximation, not per-curve ease.
- drag/longpress/key events: browser event approximation.

## Runtime-only (not mirrored in HTML)
TweenManager per-curve ease, virtual list slot reuse/variable-height compensation, NativeHost 3D/particles, event system, overlay stacking timing.

## Maintenance
- Changed showcase HTML: refresh browser.
- Nav table (NAV) is at top of loom-preview.js, add new pages there.
