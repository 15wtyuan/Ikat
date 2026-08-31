//! 帧 blob 构建器：FrameData → 拍平 blob（v15 列级增量布局）。
//! mesh 顶点 re-base 到节点本地空间（render 侧是父坐标系，减 transform.x/y）。
//!
//! v15 布局（列级增量——稳态帧带宽从每行固定 512B 降到 Skip 16B / Header 84B）：
//! - **Skip 行出 SOA**：change_level==Skip 的行只进段末 skip 段（node_id+reuse_key+
//!   flags，16B/行）——它们对后端只意味着「对象还在，清 stale」。v14 里这些行仍全量
//!   写 23 列（其中 440B 是多数节点恒零的胖参数列）。
//! - **胖参数列挪 arena**：color_matrix(80B)/effect_block(128B)/shadow_params(24B)/
//!   grad_params(208B) 不再是每行定宽列，全零 = 不写；非零块进 fat arena，行内
//!   fat_off(u32) 引用（1-based，0=无）。块的取舍是「字节全零」——与 C# 侧按 program
//!   门控读取的语义一致（无该 program 的行本就不读）。
//! - **mount_id 列（u64，0=screen）**：world-space 子树锚的行标记（子树整批挂外部
//!   世界变换），C# MirrorPool 按 mount_id 路由 SetParent。
//! - lean 列（Skip 之外的全部行）保持 SOA 随机访问：21 列定宽，84B/行。
//!
//! parked keepalive（虚拟列表休眠 slot）条目进 skip 段，flags bit1 标记——后端
//! 保留 GO 不渲染。条目集是超集（slot 根 + 后代都发），lookup miss 即 no-op。

#[allow(unused_imports)]
// BlendMode/MaskContext/NodePayload/EffectBlock 仅测试 helper 经 super::* 用。
use ikat_core::render::node::{
    BlendMode, ChangeLevel, EffectBlock, MaskContext, NodePayload, RenderNode,
};
use ikat_core::render::FrameData;
use ikat_core::scene::node::Scene;
use ikat_core::transform;

/// magic = "LOOM" little-endian。LoomGUI 时代烙印的磁盘字节，非品牌面——不动它，
/// 改名只换代码符号不换 ABI 兼容的魔数与 pkg 格式身份。
const MAGIC: u32 = 0x4D4F4F4C;
pub(crate) const VERSION: u32 = 15; // v15：列级增量（Skip 段 + fat arena + mount_id 列）

/// lean 列名 + 每元素字节数（21 列，Skip 之外的全部行；顺序 = header col_offset 序）。
///   node_id/parent_id 8B（parent_id i64，-1 = 无父）。
///   path_idx 4B（path 表 1-based 索引，0=纯色无图）。
///   change_level：0=Skip 1=Header 2=Full（lean 行只会是 1/2；Skip 行在 skip 段）。
///   mount_id：v15 新增（u64，0=screen；world-space 子树锚标记）。
///   fat_off：v15 新增（u32，1-based 指向 fat arena entry；0=无胖块）。
const LEAN_COLUMNS: &[(&str, usize)] = &[
    ("node_id", 8),
    ("parent_id", 8),
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
    ("path_idx", 4),
    ("program", 1),
    ("change_level", 1),
    ("reuse_key", 4),
    ("mount_id", 8),
    ("fat_off", 4),
];

/// lean 列下标（与 LEAN_COLUMNS 序对齐；emit 与测试共用）。
pub(crate) const COL_NODE_ID: usize = 0;
pub(crate) const COL_PARENT_ID: usize = 1;
pub(crate) const COL_VISIBLE: usize = 2;
pub(crate) const COL_ALPHA: usize = 3;
pub(crate) const COL_SORT_KEY: usize = 4;
pub(crate) const COL_MASK: usize = 5;
pub(crate) const COL_M_A: usize = 6;
pub(crate) const COL_M_B: usize = 7;
pub(crate) const COL_M_C: usize = 8;
pub(crate) const COL_M_D: usize = 9;
pub(crate) const COL_M_TX: usize = 10;
pub(crate) const COL_M_TY: usize = 11;
pub(crate) const COL_KIND: usize = 12;
pub(crate) const COL_MESH_OFF: usize = 13;
pub(crate) const COL_MESH_LEN: usize = 14;
pub(crate) const COL_PATH_IDX: usize = 15;
pub(crate) const COL_PROGRAM: usize = 16;
pub(crate) const COL_CHANGE_LEVEL: usize = 17;
pub(crate) const COL_REUSE_KEY: usize = 18;
pub(crate) const COL_MOUNT_ID: usize = 19;
pub(crate) const COL_FAT_OFF: usize = 20;

/// skip 段 entry 字节数：node_id u64 + reuse_key u32 + flags u8 + pad u8×3。
pub(crate) const SKIP_ENTRY_SIZE: usize = 16;

/// 入口：FrameData（nodes + clip 表）+ Scene（parked slot 池）→ blob 字节。
pub fn build_blob(frame: &FrameData, scene: &Scene) -> Vec<u8> {
    let nodes = &frame.nodes;
    let clips = &frame.clips;

    let num_col_offsets = LEAN_COLUMNS.len();
    let header_len = 3 * 4 // magic, version, node_count（lean+skip 总数）
        + 4 // skip_count（v15：skip 段条目数）
        + num_col_offsets * 4 // lean 列 offset
        + 2 * 4 // mesh_arena off + len
        + 2 * 4 // clip_table off + len
        + 2 * 4 // path_table off + len
        + 2 * 4; // fat_arena off + len（v15）

    // v7：path string table arena——per-frame 归一化图片 path 表。
    //   layout: path_count:u32 后跟 count × {path_len:u32, path_bytes:u8[path_len]}。
    //   path_idx（列值）1-based 索引此表：idx=0=纯色无图。
    let mut path_table_buf: Vec<u8> = Vec::new();
    path_table_buf.extend_from_slice(&0u32.to_le_bytes()); // path_count 占位，build 末回填
    let mut path_index: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    let mut mesh_arena: Vec<u8> = Vec::new();
    let mut fat_arena: Vec<u8> = Vec::new();
    // skip 段：{node_id u64, reuse_key u32, flags u8, pad 3}×count。
    let mut skip_buf: Vec<u8> = Vec::new();
    let mut col_bufs: Vec<Vec<u8>> = (0..num_col_offsets).map(|_| Vec::new()).collect();

    for rn in nodes {
        emit_render_node(
            rn,
            &mut col_bufs,
            &mut skip_buf,
            &mut mesh_arena,
            &mut fat_arena,
            &mut path_table_buf,
            &mut path_index,
        );
    }

    // parked keepalive 段：每个休眠 slot 的渲染子树（根 + 后代）都发一条极简条目
    // （进 skip 段，flags bit1=parked）。slot 根 reuse_key 是出生即定的永久 ordinal；
    // 后代 reuse_key=0（后端按 node_id 保留）。
    //
    // 子树全发（非仅根）：slot park 时 display:none 剪整子树，若只保根，后代 GO（文本
    // mesh 等）被 stale 销毁，reactivate 重建——每帧滚动 churn（item 闪没 + 掉帧）。
    // 注意：scene.lists 是 HashMap，迭代顺序跨帧不保证——skip 段排列无稳定序（无需）。
    for (slot_node, reuse_key) in scene.parked_keepalive_nodes() {
        skip_buf.extend_from_slice(&slot_node.0.to_le_bytes());
        skip_buf.extend_from_slice(&reuse_key.to_le_bytes());
        skip_buf.push(0b10); // bit1=parked，bit0=不可见
        skip_buf.extend_from_slice(&[0u8; 3]);
    }
    let skip_count = (skip_buf.len() / SKIP_ENTRY_SIZE) as u32;

    let mut off = header_len;
    let mut col_offsets: Vec<u32> = Vec::with_capacity(num_col_offsets);
    for buf in &col_bufs {
        col_offsets.push(off as u32);
        off += buf.len();
    }
    // 四 arena 顺序布局：mesh → clip → path → fat → skip。
    let mesh_arena_off = off as u32;
    let mesh_arena_len = mesh_arena.len() as u32;
    let clip_table_off = mesh_arena_off + mesh_arena_len;
    // clip 表 = clip_count:u32 + entries[count × {context_id:u32, x,y,w,h:f32, radii: 4×(rx,ry):f32}]。
    // radii 段恒写 32B（8×f32）：有圆角时为四角 (rx,ry) 对，无圆角时全零（C# 侧据全零判 CLIPPED vs CLIPPED_ROUNDED）。
    // 只含 mask_context>0 的层级（context==0 = 无 clip，永不入表）。
    const CLIP_ENTRY_SIZE: u32 = 52; // 20B(ctx+rect) + 32B(4×(rx,ry))
    let clip_count: u32 = clips.len() as u32;
    let clip_table_len = 4 + clip_count * CLIP_ENTRY_SIZE;
    let mut clip_table_buf: Vec<u8> = Vec::with_capacity(clip_table_len as usize);
    clip_table_buf.extend_from_slice(&clip_count.to_le_bytes());
    for c in clips {
        clip_table_buf.extend_from_slice(&c.context_id.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.x.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.y.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.w.to_le_bytes());
        clip_table_buf.extend_from_slice(&c.rect.h.to_le_bytes());
        // 四角半径 [TL, TR, BR, BL] 各 (rx, ry)。None → 全零（C# 判 CLIPPED）。
        let r = c.radii.unwrap_or([(0.0, 0.0); 4]);
        for &(rx, ry) in r.iter() {
            clip_table_buf.extend_from_slice(&rx.to_le_bytes());
            clip_table_buf.extend_from_slice(&ry.to_le_bytes());
        }
    }

    let path_count = path_index.len() as u32;
    path_table_buf[0..4].copy_from_slice(&path_count.to_le_bytes());
    let path_table_off = clip_table_off + clip_table_len;
    let path_table_len = path_table_buf.len() as u32; // 4 + Σ(4 + path_len)；无 path 时 = 4

    let fat_arena_off = path_table_off + path_table_len;
    let fat_arena_len = fat_arena.len() as u32;
    // skip 段位置 = fat_arena 末尾（off/len 不入 header——由 skip_count × 16B 换算）。

    // lean 行数由 node_id 列长换算（Skip 行不进列）；node_count = lean + skip 总数。
    let lean_rows = col_bufs[COL_NODE_ID].len() / 8;
    let node_count = lean_rows as u32 + skip_count;

    let mut out = Vec::new();
    out.extend_from_slice(&MAGIC.to_le_bytes());
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&node_count.to_le_bytes());
    out.extend_from_slice(&skip_count.to_le_bytes());
    for o in &col_offsets {
        out.extend_from_slice(&o.to_le_bytes());
    }
    out.extend_from_slice(&mesh_arena_off.to_le_bytes());
    out.extend_from_slice(&mesh_arena_len.to_le_bytes());
    out.extend_from_slice(&clip_table_off.to_le_bytes());
    out.extend_from_slice(&clip_table_len.to_le_bytes());
    out.extend_from_slice(&path_table_off.to_le_bytes());
    out.extend_from_slice(&path_table_len.to_le_bytes());
    out.extend_from_slice(&fat_arena_off.to_le_bytes());
    out.extend_from_slice(&fat_arena_len.to_le_bytes());
    for buf in &col_bufs {
        out.extend_from_slice(buf);
    }
    out.extend_from_slice(&mesh_arena);
    out.extend_from_slice(&clip_table_buf);
    out.extend_from_slice(&path_table_buf);
    out.extend_from_slice(&fat_arena);
    out.extend_from_slice(&skip_buf);
    out
}

/// 单 render 节点序列化：Skip → skip 段极简条目；Header/Full → lean 列
/// （mesh/fat 落对应 arena）。列写入顺序与 LEAN_COLUMNS 对齐。
#[allow(clippy::too_many_arguments)]
fn emit_render_node(
    rn: &RenderNode,
    col_bufs: &mut [Vec<u8>],
    skip_buf: &mut Vec<u8>,
    mesh_arena: &mut Vec<u8>,
    fat_arena: &mut Vec<u8>,
    path_table_buf: &mut Vec<u8>,
    path_index: &mut std::collections::HashMap<String, u32>,
) {
    let level = rn.change_level;
    if level == ChangeLevel::Skip {
        skip_buf.extend_from_slice(&rn.node_id.to_le_bytes());
        skip_buf.extend_from_slice(&rn.reuse_key.to_le_bytes());
        // flags：bit0=渲染可见（Skip 行未变，沿用后端上帧状态——写 0），bit1=parked。
        skip_buf.push(0);
        skip_buf.extend_from_slice(&[0u8; 3]);
        return;
    }
    let c = col_bufs;
    c[COL_NODE_ID].extend_from_slice(&rn.node_id.to_le_bytes());
    c[COL_PARENT_ID].extend_from_slice(
        &rn.parent_id
            .map(|p| p as i64)
            .unwrap_or(-1i64)
            .to_le_bytes(),
    );
    c[COL_VISIBLE].push(rn.visible as u8);
    c[COL_ALPHA].extend_from_slice(&rn.alpha.to_le_bytes());
    c[COL_SORT_KEY].extend_from_slice(&rn.sort_key.to_le_bytes());
    c[COL_MASK].extend_from_slice(&rn.mask_context.0.to_le_bytes());
    c[COL_M_A].extend_from_slice(&rn.world_matrix[0].to_le_bytes());
    c[COL_M_B].extend_from_slice(&rn.world_matrix[1].to_le_bytes());
    c[COL_M_C].extend_from_slice(&rn.world_matrix[2].to_le_bytes());
    c[COL_M_D].extend_from_slice(&rn.world_matrix[3].to_le_bytes());
    c[COL_M_TX].extend_from_slice(&rn.world_matrix[4].to_le_bytes());
    c[COL_M_TY].extend_from_slice(&rn.world_matrix[5].to_le_bytes());
    c[COL_CHANGE_LEVEL].push(level as u8);
    c[COL_REUSE_KEY].extend_from_slice(&rn.reuse_key.to_le_bytes());
    // v15→C8 接线：mount_id（world-space 子树锚槽位，0=screen；RenderNode.mount_root_id
    // 由 driver 分配的 u32 槽位零扩进 u64 列）。C# MirrorPool 按此路由 SetParent。
    c[COL_MOUNT_ID].extend_from_slice(&(rn.mount_root_id as u64).to_le_bytes());

    let NodePayload::Mesh {
        verts,
        uvs,
        colors,
        indices,
        image_path,
        program,
        color_matrix,
    } = &rn.payload;
    {
        c[COL_KIND].push(1);
        let path_idx = match image_path {
            Some(p) => intern_path(path_table_buf, path_index, p),
            None => 0u32,
        };
        c[COL_PATH_IDX].extend_from_slice(&path_idx.to_le_bytes());
        c[COL_PROGRAM].push(*program as u8);
        if level == ChangeLevel::Full {
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
            for col in colors {
                // alpha 剥离：colors 原样写，节点 alpha 走 _Alpha uniform（C# SetPropertyBlock）。
                mesh_arena.extend_from_slice(&col[0].to_le_bytes());
                mesh_arena.extend_from_slice(&col[1].to_le_bytes());
                mesh_arena.extend_from_slice(&col[2].to_le_bytes());
                mesh_arena.extend_from_slice(&col[3].to_le_bytes());
            }
            for ix in indices {
                mesh_arena.extend_from_slice(&(*ix).to_le_bytes());
            }
            let seg_len = mesh_arena.len() as u32 - seg_off;
            c[COL_MESH_OFF].extend_from_slice(&seg_off.to_le_bytes());
            c[COL_MESH_LEN].extend_from_slice(&seg_len.to_le_bytes());
        } else {
            // HEADER：不写 mesh arena（省带宽），off/len 占位 0。
            c[COL_MESH_OFF].extend_from_slice(&0u32.to_le_bytes());
            c[COL_MESH_LEN].extend_from_slice(&0u32.to_le_bytes());
        }
    }

    // 胖参数块：字节全零 = 不进 arena（C# 按 program 门控读取，无该 program 不读）。
    let effect_bytes = rn.effect.to_bytes();
    let shadow_bytes = shadow_params_bytes(&rn.shadow_params);
    let grad_bytes = rn.gradient.to_bytes();
    let cm_bytes = matrix_bytes(color_matrix);
    let has_cm = cm_bytes.iter().any(|&b| b != 0);
    let has_effect = effect_bytes.iter().any(|&b| b != 0);
    let has_shadow = shadow_bytes.iter().any(|&b| b != 0);
    let has_grad = grad_bytes.iter().any(|&b| b != 0);
    if has_cm || has_effect || has_shadow || has_grad {
        let off = fat_arena.len() as u32 + 1; // 1-based（0=无）
        let mut mask = 0u8;
        if has_cm {
            mask |= 0b0001;
        }
        if has_effect {
            mask |= 0b0010;
        }
        if has_shadow {
            mask |= 0b0100;
        }
        if has_grad {
            mask |= 0b1000;
        }
        fat_arena.push(mask);
        if has_cm {
            fat_arena.extend_from_slice(&cm_bytes);
        }
        if has_effect {
            fat_arena.extend_from_slice(&effect_bytes);
        }
        if has_shadow {
            fat_arena.extend_from_slice(&shadow_bytes);
        }
        if has_grad {
            fat_arena.extend_from_slice(&grad_bytes);
        }
        c[COL_FAT_OFF].extend_from_slice(&off.to_le_bytes());
    } else {
        c[COL_FAT_OFF].extend_from_slice(&0u32.to_le_bytes());
    }
}

/// [f32;6] → 24B little-endian。
fn shadow_params_bytes(p: &[f32; 6]) -> [u8; 24] {
    let mut out = [0u8; 24];
    for (i, &v) in p.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
    out
}

/// [f32;20] → 80B little-endian。
fn matrix_bytes(m: &[f32; 20]) -> [u8; 80] {
    let mut out = [0u8; 80];
    for (i, &v) in m.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
    }
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
    let idx = (path_index.len() + 1) as u32; // 1-based：首条 path → idx=1
    let bytes = path.as_bytes();
    path_table_buf.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    path_table_buf.extend_from_slice(bytes);
    path_index.insert(path.to_string(), idx);
    idx
}

#[cfg(test)]
mod tests;
