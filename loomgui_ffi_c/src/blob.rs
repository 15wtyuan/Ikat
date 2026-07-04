//! 帧 blob 构建器：FrameData → 拍平 SOA blob（§4.1）。
//! mesh 顶点 re-base 到节点本地空间（render 侧是父坐标系，减 transform.x/y）。

#[allow(unused_imports)] // BlendMode/MaskContext/NodePayload 仅测试 helper 经 super::* 用。
use loomgui_core::render::node::{BlendMode, ChangeLevel, MaskContext, NodePayload, RenderNode};
use loomgui_core::render::FrameData;
#[allow(unused_imports)] // Glyph/GlyphRun/Line/TextLayout 仅 text round-trip 测试 helper 用。
use loomgui_core::text::layout::{Glyph, GlyphRun, Line, TextLayout};
use loomgui_core::transform;

/// magic = "LOOM" little-endian。
const MAGIC: u32 = 0x4D4F4F4C;
const VERSION: u32 = 9; // v9：加 reuse_key 列（第 22 列）

/// 入口：FrameData（nodes + clip 表）→ blob 字节。
pub fn build_blob(frame: &FrameData) -> Vec<u8> {
    let nodes = &frame.nodes;
    let clips = &frame.clips;
    let n = nodes.len();
    // 列名 + 每元素字节数。v9：加 reuse_key 列（u32，第 22 列）。
    //   path_idx 占 4B（path 表 1-based 索引，0=纯色无图），22 列（v9 加 reuse_key）。
    //   v6：加 color_matrix 列（[f32;20]，80B，第 20 列）——ColorFilter。
    let columns: &[(&str, usize)] = &[
        ("node_id", 4),
        ("parent_id", 4),
        ("visible", 1),
        ("alpha", 4),
        ("sort_key", 4),
        ("mask_context", 4),
        ("m_a", 4),
        ("m_b", 4),
        ("m_c", 4),
        ("m_d", 4),
        ("m_tx", 4),
        ("m_ty", 4),
        ("payload_kind", 1),
        ("mesh_off", 4),
        ("mesh_len", 4),
        ("text_off", 4),
        ("text_len", 4),
        ("path_idx", 4), // v7：path 表 1-based 索引（0=纯色无图）
        ("program", 1),
        ("color_matrix", 80), // [f32;20] × 4 字节，第 20 列
        ("change_level", 1),  // v8：帧级变更级别（u8，0=Skip 1=Header 2=Full），第 21 列
        ("reuse_key", 4),     // v9：渲染复用键（虚拟列表 slot key），第 22 列
    ];
    let num_col_offsets = columns.len(); // 22
    let header_len = 3 * 4                          // magic, version, node_count
        + num_col_offsets * 4                       // 列 offset（21）
        + 2 * 4                                     // mesh_arena off + len
        + 2 * 4                                     // text_arena off + len
        + 2 * 4                                     // clip_table off + len
        + 2 * 4; // path_table off + len（v7 新增）

    // 先把 mesh arena + text arena + per-node 列值算出来
    // （mesh/text arena 决定列值里的 mesh_off/len 与 text_off/len）。
    let mut mesh_arena: Vec<u8> = Vec::new();
    let mut text_arena: Vec<u8> = Vec::new(); // Text 节点 layout（§4.1）
                                              // v7：path string table arena——per-frame 归一化图片 path 表（§5.2）。
                                              //   layout: path_count:u32 后跟 count × {path_len:u32, path_bytes:u8[path_len]}。
                                              //   path_idx（列值）1-based 索引此表：idx=0=纯色无图，idx>0 = 第 idx 条 path。
                                              //   build 期间用 path_index map 去重 intern（同 path 复用同一 idx，节省 arena）。
    let mut path_table_buf: Vec<u8> = Vec::new();
    path_table_buf.extend_from_slice(&0u32.to_le_bytes()); // path_count 占位，build 末回填
    let mut path_index: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
    let mut col_node_id = Vec::<u8>::new();
    let mut col_parent_id = Vec::<u8>::new();
    let mut col_visible = Vec::<u8>::new();
    let mut col_alpha = Vec::<u8>::new();
    let mut col_sort_key = Vec::<u8>::new();
    let mut col_mask = Vec::<u8>::new();
    let mut col_ma = Vec::<u8>::new();
    let mut col_mb = Vec::<u8>::new();
    let mut col_mc = Vec::<u8>::new();
    let mut col_md = Vec::<u8>::new();
    let mut col_mtx = Vec::<u8>::new();
    let mut col_mty = Vec::<u8>::new();
    let mut col_kind = Vec::<u8>::new();
    let mut col_mesh_off = Vec::<u8>::new();
    let mut col_mesh_len = Vec::<u8>::new();
    let mut col_text_off = Vec::<u8>::new();
    let mut col_text_len = Vec::<u8>::new();
    let mut col_path_idx = Vec::<u8>::new(); // v7：path_idx 列
    let mut col_program = Vec::<u8>::new();
    let mut col_color_matrix = Vec::<u8>::new();
    let mut col_change_level = Vec::<u8>::new();
    let mut col_reuse_key = Vec::<u8>::new();

    for rn in nodes {
        col_node_id.extend_from_slice(&rn.node_id.to_le_bytes());
        col_parent_id
            .extend_from_slice(&rn.parent_id.map(|p| p as i32).unwrap_or(-1).to_le_bytes());
        col_visible.push(rn.visible as u8);
        col_alpha.extend_from_slice(&rn.alpha.to_le_bytes());
        col_sort_key.extend_from_slice(&rn.sort_key.to_le_bytes());
        col_mask.extend_from_slice(&rn.mask_context.0.to_le_bytes());
        col_ma.extend_from_slice(&rn.world_matrix[0].to_le_bytes());
        col_mb.extend_from_slice(&rn.world_matrix[1].to_le_bytes());
        col_mc.extend_from_slice(&rn.world_matrix[2].to_le_bytes());
        col_md.extend_from_slice(&rn.world_matrix[3].to_le_bytes());
        col_mtx.extend_from_slice(&rn.world_matrix[4].to_le_bytes());
        col_mty.extend_from_slice(&rn.world_matrix[5].to_le_bytes());

        // text_off/text_len 每节点都写——Text 节点指向 text_arena 内实段，
        // 其余节点占位 0（match 各 arm 内 push 进 col_text_off/len）。

        col_change_level.push(rn.change_level as u8);
        col_reuse_key.extend_from_slice(&rn.reuse_key.to_le_bytes());
        let write_arena = matches!(rn.change_level, ChangeLevel::Full);

        match &rn.payload {
            NodePayload::Mesh {
                verts,
                uvs,
                colors,
                indices,
                image_path,
                program,
                color_matrix,
            } => {
                col_kind.push(1);
                // v7：把 image_path intern 进 path string table，写 1-based path_idx。
                //   None（纯色）→ 0；Some(p) → p 在 path 表里的 1-based 索引。
                let path_idx = match image_path {
                    Some(p) => intern_path(&mut path_table_buf, &mut path_index, p),
                    None => 0u32,
                };
                col_path_idx.extend_from_slice(&path_idx.to_le_bytes());
                col_program.push(*program as u8);
                for &v in color_matrix.iter() {
                    col_color_matrix.extend_from_slice(&v.to_le_bytes());
                }
                if write_arena {
                    // v4：re-base 顶点两路径。纯平移 → 减 (tx,ty) 得本地；
                    // 非纯平移 → 顶点已 box 本地 → 不减。
                    let pure = transform::is_pure_translation(&rn.world_matrix);
                    let (tx, ty) = if pure {
                        (rn.world_matrix[4], rn.world_matrix[5])
                    } else {
                        (0.0, 0.0)
                    };
                    let seg_off = mesh_arena.len() as u32;
                    mesh_arena.extend_from_slice(&(verts.len() as u32).to_le_bytes());
                    mesh_arena.extend_from_slice(&(indices.len() as u32).to_le_bytes());
                    for v in verts {
                        mesh_arena.extend_from_slice(&(v[0] - tx).to_le_bytes());
                        mesh_arena.extend_from_slice(&(v[1] - ty).to_le_bytes());
                    }
                    for u in uvs {
                        mesh_arena.extend_from_slice(&u[0].to_le_bytes());
                        mesh_arena.extend_from_slice(&u[1].to_le_bytes());
                    }
                    for c in colors {
                        // alpha 剥离：colors 原样写，节点 alpha 走 _Alpha uniform（C# SetPropertyBlock）。
                        mesh_arena.extend_from_slice(&c[0].to_le_bytes());
                        mesh_arena.extend_from_slice(&c[1].to_le_bytes());
                        mesh_arena.extend_from_slice(&c[2].to_le_bytes());
                        mesh_arena.extend_from_slice(&c[3].to_le_bytes());
                    }
                    for ix in indices {
                        mesh_arena.extend_from_slice(&(*ix as u32).to_le_bytes());
                    }
                    let seg_len = mesh_arena.len() as u32 - seg_off;
                    col_mesh_off.extend_from_slice(&seg_off.to_le_bytes());
                    col_mesh_len.extend_from_slice(&seg_len.to_le_bytes());
                } else {
                    // SKIP/HEADER：不写 mesh arena（省带宽），off/len 占位 0。
                    col_mesh_off.extend_from_slice(&0u32.to_le_bytes());
                    col_mesh_len.extend_from_slice(&0u32.to_le_bytes());
                }
                // Mesh 节点无 text 段：text_off/len 占位 0。
                col_text_off.extend_from_slice(&0u32.to_le_bytes());
                col_text_len.extend_from_slice(&0u32.to_le_bytes());
            }
            NodePayload::Text {
                layout,
                font_size,
                color,
                program,
            } => {
                col_kind.push(2);
                col_program.push(*program as u8);
                for _ in 0..20 {
                    col_color_matrix.extend_from_slice(&0f32.to_le_bytes());
                }
                col_path_idx.extend_from_slice(&0u32.to_le_bytes());
                col_mesh_off.extend_from_slice(&0u32.to_le_bytes());
                col_mesh_len.extend_from_slice(&0u32.to_le_bytes());

                if write_arena {
                    let seg_off = text_arena.len() as u32;
                    text_arena.extend_from_slice(&(*font_size as u32).to_le_bytes());
                    for &c in color {
                        text_arena.extend_from_slice(&c.to_le_bytes());
                    }
                    let glyphs_start = text_arena.len();
                    text_arena.extend_from_slice(&0u32.to_le_bytes()); // glyph_count 占位
                    let mut count = 0u32;
                    for line in &layout.lines {
                        // pen_y = line.baseline（绝对 y，layout.rs:43 已含行偏移 line_y）。
                        // 勿叠加 line.y（= line_y）——否则每行多一个行高，行距翻倍（多行才暴露）。
                        let pen_y = line.baseline;
                        for run in &line.runs {
                            for g in &run.glyphs {
                                text_arena.extend_from_slice(&g.codepoint.to_le_bytes());
                                text_arena.extend_from_slice(&g.x.to_le_bytes());
                                text_arena.extend_from_slice(&pen_y.to_le_bytes());
                                count += 1;
                            }
                        }
                    }
                    text_arena[glyphs_start..glyphs_start + 4]
                        .copy_from_slice(&count.to_le_bytes());
                    let seg_len = text_arena.len() as u32 - seg_off;
                    col_text_off.extend_from_slice(&seg_off.to_le_bytes());
                    col_text_len.extend_from_slice(&seg_len.to_le_bytes());
                } else {
                    // SKIP/HEADER：不写 text arena，off/len 占位 0。
                    col_text_off.extend_from_slice(&0u32.to_le_bytes());
                    col_text_len.extend_from_slice(&0u32.to_le_bytes());
                }
            }
        }
    }

    let col_bufs: Vec<(&str, &Vec<u8>)> = vec![
        ("node_id", &col_node_id),
        ("parent_id", &col_parent_id),
        ("visible", &col_visible),
        ("alpha", &col_alpha),
        ("sort_key", &col_sort_key),
        ("mask_context", &col_mask),
        ("m_a", &col_ma),
        ("m_b", &col_mb),
        ("m_c", &col_mc),
        ("m_d", &col_md),
        ("m_tx", &col_mtx),
        ("m_ty", &col_mty),
        ("payload_kind", &col_kind),
        ("mesh_off", &col_mesh_off),
        ("mesh_len", &col_mesh_len),
        ("text_off", &col_text_off),
        ("text_len", &col_text_len),
        ("path_idx", &col_path_idx), // v7：path 表 1-based 索引
        ("program", &col_program),
        ("color_matrix", &col_color_matrix),
        ("change_level", &col_change_level),
        ("reuse_key", &col_reuse_key),
    ];

    // 算各列 offset。
    let mut off = header_len;
    let mut col_offsets: Vec<u32> = Vec::new();
    for (_name, buf) in &col_bufs {
        col_offsets.push(off as u32);
        off += buf.len();
    }
    // 三 arena header offset。text_arena 紧跟 mesh_arena；无 clip 时 clip 表仅 clip_count(u32)=0。
    let mesh_arena_off = off as u32;
    let mesh_arena_len = mesh_arena.len() as u32;
    let text_arena_off = mesh_arena_off + mesh_arena_len;
    let text_arena_len = text_arena.len() as u32; // Text 节点 layout 序列化进 text_arena
    let clip_table_off = text_arena_off + text_arena_len;
    // clip 表 = clip_count:u32 + entries[count × {context_id:u32, x,y,w,h:f32}]（20B/entry）。
    // 只含 mask_context>0 的层级（context==0 = 无 clip，永不入表）。§4.4 / §4.1。
    let clip_count: u32 = clips.len() as u32;
    let clip_table_len: u32 = 4 + clip_count * 20;
    let mut clip_table_buf: Vec<u8> = Vec::with_capacity(clip_table_len as usize);
    clip_table_buf.extend_from_slice(&clip_count.to_le_bytes());
    for c in clips {
        clip_table_buf.extend_from_slice(&c.context_id.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.x.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.y.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.w.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.h.to_le_bytes());
    }

    // v7：path string table arena 紧跟 clip_table 末段（稳定布局，文档化于 §5.2）。
    //   path_count（path_table_buf 首 4B）现已确定——回填实 count；再算 off/len。
    let path_count = path_index.len() as u32;
    path_table_buf[0..4].copy_from_slice(&path_count.to_le_bytes());
    let path_table_off = clip_table_off + clip_table_len;
    let path_table_len = path_table_buf.len() as u32; // 4 + Σ(4 + path_len)；无 path 时 = 4（仅 count=0）

    // 拼装。
    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&(n as u32).to_le_bytes());
    for o in &col_offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&mesh_arena_off.to_le_bytes());
    out.extend_from_slice(&mesh_arena_len.to_le_bytes());
    out.extend_from_slice(&text_arena_off.to_le_bytes());
    out.extend_from_slice(&text_arena_len.to_le_bytes());
    out.extend_from_slice(&clip_table_off.to_le_bytes());
    out.extend_from_slice(&clip_table_len.to_le_bytes());
    out.extend_from_slice(&path_table_off.to_le_bytes()); // v7：path_table off + len
    out.extend_from_slice(&path_table_len.to_le_bytes());
    for (_name, buf) in &col_bufs {
        out.extend_from_slice(buf);
    }
    out.extend_from_slice(&mesh_arena);
    out.extend_from_slice(&text_arena);
    // clip 表：clip_count + entries。
    out.extend_from_slice(&clip_table_buf);
    // v7：path string table arena（blob 末段）。
    out.extend_from_slice(&path_table_buf);
    out
}

/// v7：把 path intern 进 path string table arena，返回 1-based 索引。
/// 同 path 复用同一 idx（去重；path_index map 跨整帧 build）。
/// path_table_buf 布局：path_count:u32（首 4B，build 末回填）后跟
///   count × {path_len:u32, path_bytes:u8[path_len]}（length-prefixed UTF-8）。
fn intern_path(
    path_table_buf: &mut Vec<u8>,
    path_index: &mut std::collections::HashMap<String, u32>,
    path: &str,
) -> u32 {
    if let Some(&idx) = path_index.get(path) {
        return idx;
    }
    // 新 path：追加 {path_len, path_bytes}，分配下一 1-based idx。
    let idx = (path_index.len() + 1) as u32; // 1-based：首条 path → idx=1
    let bytes = path.as_bytes();
    path_table_buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    path_table_buf.extend_from_slice(bytes);
    path_index.insert(path.to_string(), idx);
    idx
}

#[cfg(test)]
mod tests;
