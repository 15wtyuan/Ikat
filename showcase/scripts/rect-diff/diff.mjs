// rect-diff: compare browser rect JSON (browser-rect.mjs) vs core rect JSON
// (dump_page --json). Both arrays have the same element shape
// {domIndex, tag, id, classes, x, y, w, h}, but the two enumerations are NOT
// index-aligned, so domIndex alone is unreliable as a pairing key.
//
// Why domIndex misaligns: core DFS may still differ from browser
// `querySelectorAll('body *')` — component wrappers injected around the user
// tree, TextNodes that core emits but `body *` excludes, and other
// enumeration-order quirks. Any such offset would generate false positives
// under naive domIndex pairing.
//
// Pairing strategy:
//   1. PRIMARY: pair by `id` when both sides expose a non-null id that matches.
//   2. FALLBACK for idless nodes: bucket by `tag|sorted-classes` and pair
//      index-aligned within each bucket (DOM/DFS order is preserved per
//      bucket). Buckets whose counts disagree pair the common prefix and
//      surface the remainder as `idless-unpaired`.
//
// Usage: node diff.mjs <browser.json> <core.json> [--tol-box=N] [--tol-text=N]
//   --tol-box  (default 1) rect tolerance for non-text elements
//   --tol-text (default 3) rect tolerance for span / #text (font-metric drift)
// Exit code: 1 if any diff / unmatched, else 0 (idless-unpaired is informational —
// core's component wrappers always appear core-side only).

import { readFileSync } from 'fs';

function parseArgs(argv) {
  const positional = [];
  const opts = {};
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a.startsWith('--')) {
      const eq = a.indexOf('=');
      const k = eq === -1 ? a.slice(2) : a.slice(2, eq);
      const v = eq === -1 ? 'true' : a.slice(eq + 1);
      opts[k] = v;
    } else {
      positional.push(a);
    }
  }
  return { positional, opts };
}

const { positional, opts } = parseArgs(process.argv);
if (positional.length < 2) {
  console.error('usage: node diff.mjs <browser.json> <core.json> [--tol-box=N] [--tol-text=N]');
  process.exit(2);
}

const [browserPath, corePath] = positional;
const boxTol = Number(opts['tol-box'] ?? 1);
const textTol = Number(opts['tol-text'] ?? 3);
if (!Number.isFinite(boxTol) || !Number.isFinite(textTol)) {
  console.error('error: --tol-box and --tol-text must be numeric');
  process.exit(2);
}

// Filter core #text whitespace nodes: browser `body *` only enumerates elements,
// so any core #text would otherwise surface as core-only unmatched noise.
// Browser side never contains #text; the filter is harmless there.
const browserAll = JSON.parse(readFileSync(browserPath, 'utf8'))
  .filter((e) => e && e.tag !== '#text');
const coreAll = JSON.parse(readFileSync(corePath, 'utf8'))
  .filter((e) => e && e.tag !== '#text');

// 0x0 boxes (display:none / parked template slots / collapsed) are enumerated
// differently per side — the browser lists every DOM element including hidden
// ones, core only emits what it laid out — so feeding them into the idless
// index-aligned buckets misaligns every later pair in the bucket (one extra
// hidden option in the browser shifts all real pairs). Drop browser-side 0x0
// entirely; on the core side keep 0x0 spans — those are rich-text folded
// inlines (public tree keeps the node, rect collapses to 0x0 by design) that
// must still pair with the browser's real inline box so comparePair can
// classify them as FOLDED instead of leaving them unpaired.
const bZero = browserAll.filter((e) => e.w === 0 && e.h === 0 && !e.id);
const cZero = coreAll.filter(
  (e) => e.w === 0 && e.h === 0 && !e.id && e.tag !== 'span'
);
const browserPairable = browserAll.filter((e) => !(e.w === 0 && e.h === 0 && !e.id));
const corePairable = coreAll.filter(
  (e) => !(e.w === 0 && e.h === 0 && !e.id && e.tag !== 'span')
);

// Partition into id-keyed map and idless buckets keyed by tag+sorted-classes.
// Classes are sorted so {row,card} and {card,row} collapse to the same bucket.
const bucketKey = (e) => `${e.tag}|${[...(e.classes ?? [])].sort().join(',')}`;

function partition(els) {
  const byId = new Map();
  const idless = new Map();
  for (const e of els) {
    if (e.id) {
      byId.set(e.id, e);
    } else {
      const k = bucketKey(e);
      if (!idless.has(k)) idless.set(k, []);
      idless.get(k).push(e);
    }
  }
  return { byId, idless };
}

const b = partition(browserPairable);
const c = partition(corePairable);

const FIELDS = ['x', 'y', 'w', 'h'];

// Readable single-line label for an element. For pairs we prefer the id; for
// idless / unmatched we include the source side + domIndex so the user can
// locate the entry in the source JSON.
function label(e, side) {
  const cls = (e.classes ?? []).join('.');
  const tagCls = cls ? `${e.tag}.${cls}` : e.tag;
  if (e.id) return e.id;
  return `<${tagCls}>[${side}#${e.domIndex}]`;
}

const diffLines = [];
const unmatchedLines = [];
const idlessUnpairedLines = [];
const foldedLines = [];
let diffCount = 0;

function comparePair(bEl, cEl) {
  // Rich-text inline spans fold into the parent block run in core (task-1 text
  // model): the public tree keeps the span's id but its rect is 0x0 by design
  // — the browser measures it as a real inline box. Report as informational
  // FOLDED, not a diff; there is nothing to align.
  const coreEmpty = cEl.w === 0 && cEl.h === 0;
  if (coreEmpty && (cEl.tag === 'span' || cEl.tag === '#text')) {
    foldedLines.push(`${label(bEl, 'b')}: folded inline span (core rect 0x0 by design, browser ${Math.round(bEl.w)}x${Math.round(bEl.h)})`);
    return;
  }
  // Text elements (span = TextElement, the visible-text container; #text is
  // pre-filtered but kept here defensively) get the wider text tolerance to
  // absorb font-metric drift between browser fonts and LoomGUI runtime shaping.
  const isText = cEl.tag === 'span' || cEl.tag === '#text';
  const tol = isText ? textTol : boxTol;
  const tag = label(bEl, 'b');
  // A 0x0 box (display:none / collapsed) has no meaningful position: Chromium
  // reports origin (0,0) while core reports the parent content-box origin.
  // Skip x/y when either side is 0-size; w/h is always compared so a genuine
  // visible-vs-hidden collapse still surfaces as a real diff.
  const bEmpty = bEl.w === 0 && bEl.h === 0;
  for (const f of FIELDS) {
    if ((f === 'x' || f === 'y') && (bEmpty || coreEmpty)) continue;
    const bv = bEl[f];
    const cv = cEl[f];
    if (typeof bv !== 'number' || typeof cv !== 'number') continue;
    if (Math.abs(bv - cv) > tol) {
      diffCount++;
      diffLines.push(`DIFF ${tag}.${f}: browser=${bv} core=${cv} (tol=${tol})`);
    }
  }
}

// 1) id-primary pairing.
for (const [id, bEl] of b.byId) {
  const cEl = c.byId.get(id);
  if (cEl) comparePair(bEl, cEl);
  else unmatchedLines.push(`${id}: browser-only`);
}
for (const [id] of c.byId) {
  if (!b.byId.has(id)) unmatchedLines.push(`${id}: core-only`);
}

// 2) idless bucket pairing — pair index-aligned within each tag+classes bucket
//    (DOM/DFS order is preserved per bucket). Remainder from unequal bucket
//    sizes surfaces as idless-unpaired.
const allBuckets = new Set([...b.idless.keys(), ...c.idless.keys()]);
for (const k of allBuckets) {
  const bList = b.idless.get(k) ?? [];
  const cList = c.idless.get(k) ?? [];
  const pairCount = Math.min(bList.length, cList.length);
  for (let i = 0; i < pairCount; i++) comparePair(bList[i], cList[i]);
  const bRem = bList.slice(pairCount);
  const cRem = cList.slice(pairCount);
  for (const e of bRem) idlessUnpairedLines.push(`${label(e, 'b')}: browser-only (idless bucket ${k})`);
  // Core-side 0x0 remainders stay silent: they are hidden-panel / parked /
  // folded spans whose browser counterparts were already excluded by the 0x0
  // filter above, so an unpaired line here is structural, not layout signal.
  for (const e of cRem) {
    if (e.w === 0 && e.h === 0) continue;
    idlessUnpairedLines.push(`${label(e, 'c')}: core-only (idless bucket ${k})`);
  }
}

// Grouped output for readability.
if (diffLines.length) {
  console.log('--- DIFFS (rect fields beyond tolerance) ---');
  for (const l of diffLines) console.log(l);
}
if (unmatchedLines.length) {
  console.log('--- UNMATCHED (id present on one side only) ---');
  for (const l of unmatchedLines) console.log(l);
}
if (idlessUnpairedLines.length) {
  console.log('--- IDLESS-UNPAIRED (no id; tag+classes bucket count mismatch) ---');
  for (const l of idlessUnpairedLines) console.log(l);
}
if (foldedLines.length) {
  console.log('--- FOLDED (core rich-text inline spans; rect 0x0 by design) ---');
  for (const l of foldedLines) console.log(l);
}
if (bZero.length || cZero.length) {
  console.log(`(0x0 idless boxes excluded from pairing: browser ${bZero.length}, core ${cZero.length})`);
}

// Exit code deliberately excludes idless-unpaired: a non-zero count there is
// structurally expected (core's component wrappers, domIndex
// offset) and is reported as informational, not a gate failure. Only rect
// DIFFS on paired elements and id-mismatched UNMATCHED entries fail the gate.
const failing = diffCount + unmatchedLines.length;
console.log(
  `\nsummary: ${diffCount} rect diffs, ${unmatchedLines.length} unmatched, ${idlessUnpairedLines.length} idless-unpaired, ${foldedLines.length} folded`
);
process.exit(failing > 0 ? 1 : 0);
