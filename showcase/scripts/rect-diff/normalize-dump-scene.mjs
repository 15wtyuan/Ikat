// Normalize a Unity PlayMode DumpSceneJson export into the flat rect shape
// diff.mjs consumes ({domIndex, tag, id, classes[], x, y, w, h}).
//
// Source shape (core dump_scene_json, via IkatHost.DumpSceneJson /
// IkatBridge.DumpScene() on the Unity machine):
//   {node_id, parent, tag, id, classes: "a b" (space-joined string),
//    kind, layout: {x,y,w,h}, world_matrix[6], anim_tr, anim_op, visible}
//
// The diagnostic tag column diverges from the rect-diff semantic vocabulary
// in three places (ListView->div, CustomElement->div, TextNode->span), so the
// `kind` field decides the tag here — it is lossless and matches what
// browser-rect.mjs's role normalization produces on the other side.
//
// Usage: node normalize-dump-scene.mjs <dump-scene.json> <out.json>

import { readFileSync, writeFileSync } from 'fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [, , inPath, outPath] = process.argv;
if (!inPath || !outPath) {
  console.error('usage: node normalize-dump-scene.mjs <dump-scene.json> <out.json>');
  process.exit(2);
}

// kind→tag 表来自 Rust 单源导出（semantic-tags.json，ikat_pkg 测试钉新鲜度，
// 真相源 = core kind_to_html_tag / ALL_NODE_KINDS）。
const KIND_TAG = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), 'semantic-tags.json'), 'utf8'),
).kind;

const nodes = JSON.parse(readFileSync(inPath, 'utf8'));
const out = nodes.map((n, i) => ({
  domIndex: i,
  // CustomElement：dump 的 tag 字段是 custom_tag hyphen 字面量（pkg v35 展开保留），
  // 与浏览器侧 tagName 原文配对；其余 kind 走 KIND_TAG（dump 的诊断 tag 有三处偏离）。
  tag:
    (n.kind === 'CustomElement' ? n.tag : undefined) ??
    KIND_TAG[n.kind] ??
    n.tag ??
    'div',
  id: n.id || null,
  classes: (n.classes ?? '').split(/\s+/).filter(Boolean),
  x: n.layout?.x ?? 0,
  y: n.layout?.y ?? 0,
  w: n.layout?.w ?? 0,
  h: n.layout?.h ?? 0,
}));

writeFileSync(outPath, JSON.stringify(out, null, 2));
console.log(`normalized ${out.length} nodes -> ${outPath}`);
