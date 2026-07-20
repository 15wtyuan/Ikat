// rect-diff: compare browser rect JSON (browser-rect.mjs) vs core rect JSON
// (spec4b_dump --json). Both arrays have the same element shape
// {domIndex, tag, id, classes, x, y, w, h}, but the two enumerations are NOT
// index-aligned, so domIndex alone is unreliable as a pairing key.
//
// Why domIndex misaligns: core DFS emits the implicit root + a component
// wrapper before the user tree, so core's element N typically sits at domIndex
// ~N+2 vs browser's element N. Browser `querySelectorAll('body *')` also
// excludes TextNodes, while core DFS includes inter-element whitespace
// TextNodes (~20 of them in spec4b). Both gaps would generate false positives
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
// core's implicit root + component wrappers always appear core-side only).

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

const b = partition(browserAll);
const c = partition(coreAll);

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
let diffCount = 0;

function comparePair(bEl, cEl) {
  // Text elements (span = TextElement, the visible-text container; #text is
  // pre-filtered but kept here defensively) get the wider text tolerance to
  // absorb font-metric drift between browser fonts and LoomGUI runtime shaping.
  const isText = cEl.tag === 'span' || cEl.tag === '#text';
  const tol = isText ? textTol : boxTol;
  const tag = label(bEl, 'b');
  for (const f of FIELDS) {
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
  for (const e of cRem) idlessUnpairedLines.push(`${label(e, 'c')}: core-only (idless bucket ${k})`);
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

// Exit code deliberately excludes idless-unpaired: a non-zero count there is
// structurally expected (core's implicit root + component wrappers, domIndex
// offset) and is reported as informational, not a gate failure. Only rect
// DIFFS on paired elements and id-mismatched UNMATCHED entries fail the gate.
const failing = diffCount + unmatchedLines.length;
console.log(
  `\nsummary: ${diffCount} rect diffs, ${unmatchedLines.length} unmatched, ${idlessUnpairedLines.length} idless-unpaired`
);
process.exit(failing > 0 ? 1 : 0);
