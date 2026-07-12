//! 字形 atlas：核心自绘字体的字形位图缓存 + 货架分配。
//!
//! 单物理 etagere 实例（每页一个），多页溢出。key=(font_id, glyph_id)——单一 SDF：
//! 所有 target size 共享同一份固定 SOURCE_SIZE 光栅的 SDF，size 不进 key。etagere
//! 增量 allocate 不 repack → 旧字形 UV 永不变（每帧上传只动新分配槽，旧槽原地不动），
//! 后端按 page_bytes + dirty_pages 增量上传即可。
//!
//! measure_text（layout.rs）保持纯函数不读本结构；atlas 为 Stage 持有，渲染 build
//! 期查询。

use std::collections::HashMap;

use ab_glyph_rasterizer::{point, Point, Rasterizer};
use etagere::AtlasAllocator;
use ttf_parser::{Face, GlyphId, OutlineBuilder};

/// 4096² R8 = 16MB / 页。经验上容 3000-4000 CJK 字形 @48px；后续任务接 multi-page
/// 溢出策略时仍以此为单页上限。
const PAGE_SIZE: i32 = 4096;

/// SDF source 光栅尺寸（design px）。所有 target size 共享一份此尺寸的 SDF，
/// 运行时 quad 按 target/SOURCE_SIZE 缩放。TMP 中档值，CJK 笔画清晰、放大 72+ 仍锐利。
pub const SOURCE_SIZE: u16 = 48;
/// SDF spread：atlas 里每像素存 ±SPREAD 范围的 signed distance。
/// SDF 硬约束 = spread 须 ≥ 最大 effect 宽度（spread 之外 distance 饱和、effect 硬切）。
/// 12 覆盖 showcase text-shadow blur 12px。字形 bitmap 四周各扩 SPREAD px。
pub const SPREAD: i32 = 12;

/// 字形位图四周的像素 padding。SDF 模式下 = SPREAD：bitmap 四周各扩 SPREAD px
/// 容纳距离场（spread 之外 distance 饱和到 0/255）；同时让相邻字形在 atlas 内不紧贴。
/// pub：build_text_mesh 算 quad 顶点时需据此把位图原点（bbox 外扩 pad）对齐——
/// bearing 来自 bbox，而 px_w/px_h/UV 含 pad，定位须减 pad 才不偏。
pub const GLYPH_PAD: i32 = SPREAD;

/// SDF 超采样倍数（每边）。hi-res(×N) 光栅 coverage → hi-res 二值 mask → hi-res
/// 8SSEDT → 下采样 SDF 回 1x，把 zero-crossing 精度提升 N 倍，消除放大时斜边的
/// 位图锯齿波浪。关键 = EDT 必须在 hi-res 跑（zero-crossing 才是 hi-res 精度）；
/// 仅光栅超采样无用——ab_glyph coverage 本就是精确面积覆盖率，1x 二值化丢的是
/// 像素分辨率而非 coverage 精度。对标 TextMeshPro 的 SDF16 档（16x oversampling）。
const OVERSAMPLE: u32 = 4;

/// 字形缓存键。单一 SDF：所有 size 共享一份 source SDF，size 不进 key。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphKey {
    pub font_id: u32,
    pub glyph_id: u16,
}

/// 字形在其所在页上的归一化 UV（[0..1]）+ 光栅像素尺寸。
/// px_w/px_h 供 build_text_mesh 算 quad 几何尺寸，不经 atlas 尺寸反推（直接来自光栅结果更可靠）。
#[derive(Debug, Clone, Copy)]
pub struct GlyphRect {
    pub page: u32,
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub px_w: u32,
    pub px_h: u32,
}

/// 内部像素空间分配结果：页号 + 槽位左上角像素坐标。
struct AllocRect {
    page: u32,
    px_x: i32,
    px_y: i32,
}

/// 单张 atlas 页：货架分配器 + R8 像素缓冲。
struct AtlasPage {
    allocator: AtlasAllocator,
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl AtlasPage {
    fn new(width: u32, height: u32) -> Self {
        AtlasPage {
            allocator: AtlasAllocator::new(etagere::size2(width as i32, height as i32)),
            width,
            height,
            pixels: vec![0u8; (width as usize) * (height as usize)],
        }
    }
}

/// 字形 atlas：page 列表 + 键→UV 缓存 + 脏页号集合。
pub struct GlyphAtlas {
    pages: Vec<AtlasPage>,
    cache: HashMap<GlyphKey, GlyphRect>,
    dirty: Vec<u32>,
}

impl Default for GlyphAtlas {
    fn default() -> Self {
        Self::new()
    }
}

impl GlyphAtlas {
    pub fn new() -> Self {
        GlyphAtlas {
            pages: Vec::new(),
            cache: HashMap::new(),
            dirty: Vec::new(),
        }
    }

    /// 光栅 + 分配 + blit 一个字形，返回其 UV。同 key 二次调用走缓存（命中既不光栅
    /// 也不分配），保证整个 atlas 进程内同 key 返同 UV（etagere 不 repack）。
    pub fn ensure(&mut self, face: &Face<'_>, key: GlyphKey) -> GlyphRect {
        if let Some(r) = self.cache.get(&key) {
            return *r;
        }
        let (pixels, gw, gh) = rasterize_glyph(face, key.glyph_id);
        if gw == 0 || gh == 0 {
            // 无轮廓字形（空格等）：不分配槽位、不 blit、不标脏。缓存空 rect 让后续
            // 命中直接跳过；build_text_mesh 据 px_w/px_h==0 跳过 quad（advance 在 layout 已算）。
            let empty = GlyphRect {
                page: 0,
                u0: 0.0,
                v0: 0.0,
                u1: 0.0,
                v1: 0.0,
                px_w: 0,
                px_h: 0,
            };
            self.cache.insert(key, empty);
            return empty;
        }
        let alloc = self.allocate(gw, gh);

        // borrow：先把 page 索引拿出，避免与 self.pages 的可变借用冲突。
        let page = &mut self.pages[alloc.page as usize];
        let stride = page.width as usize;
        let px_x = alloc.px_x as usize;
        let px_y = alloc.px_y as usize;
        for y in 0..gh as usize {
            for x in 0..gw as usize {
                let dst = (px_y + y) * stride + (px_x + x);
                let src = y * gw as usize + x;
                page.pixels[dst] = pixels[src];
            }
        }

        if !self.dirty.contains(&alloc.page) {
            self.dirty.push(alloc.page);
        }

        let width_f = page.width as f32;
        let height_f = page.height as f32;
        let uv = GlyphRect {
            page: alloc.page,
            u0: px_x as f32 / width_f,
            v0: px_y as f32 / height_f,
            u1: (px_x + gw as usize) as f32 / width_f,
            v1: (px_y + gh as usize) as f32 / height_f,
            px_w: gw,
            px_h: gh,
        };
        self.cache.insert(key, uv);
        uv
    }

    /// 自上次 clear_dirty 以来收过新字形的页号。后端据此增量重传 GPU。
    pub fn dirty_pages(&self) -> &[u32] {
        &self.dirty
    }

    /// 标记脏页已上传。通常在 backend pull 完 page_bytes 之后调。
    pub fn clear_dirty(&mut self) {
        self.dirty.clear();
    }

    /// 取某页的 R8 像素 + 宽高，供后端上传纹理。
    /// page 越界返空切片（`(&[], 0, 0)`），保证 FFI 不 panic（FFI 入门 page 不可信）。
    pub fn page_bytes(&self, page: u32) -> (&[u8], u32, u32) {
        match self.pages.get(page as usize) {
            Some(p) => (&p.pixels, p.width, p.height),
            None => (&[], 0, 0),
        }
    }

    /// 取一个 1×1 白像素槽（装饰线 / 纯色填充用）。首次调用分配并填 255，其后命中
    /// 缓存。用 sentinel key（font_id=u32::MAX, glyph_id=u16::MAX）避与真字形键空间碰撞。
    pub fn ensure_solid(&mut self) -> GlyphRect {
        let key = GlyphKey {
            font_id: u32::MAX,
            glyph_id: u16::MAX,
        };
        if let Some(r) = self.cache.get(&key) {
            return *r;
        }
        let alloc = self.allocate(1, 1);
        let page = &mut self.pages[alloc.page as usize];
        page.pixels[alloc.px_y as usize * page.width as usize + alloc.px_x as usize] = 255;
        if !self.dirty.contains(&alloc.page) {
            self.dirty.push(alloc.page);
        }
        let uv = GlyphRect {
            page: alloc.page,
            u0: alloc.px_x as f32 / page.width as f32,
            v0: alloc.px_y as f32 / page.height as f32,
            u1: (alloc.px_x + 1) as f32 / page.width as f32,
            v1: (alloc.px_y + 1) as f32 / page.height as f32,
            px_w: 1,
            px_h: 1,
        };
        self.cache.insert(key, uv);
        uv
    }

    /// 在已存在页中找第一个能容下 w×h 的；都不够就新开一页。返回槽位的像素原点。
    /// 多页是溢出兜底：单页 4096² 装满后追加第二页，调用方不感知页上限。
    fn allocate(&mut self, gw: u32, gh: u32) -> AllocRect {
        let want = etagere::size2(gw as i32, gh as i32);
        for (idx, page) in self.pages.iter_mut().enumerate() {
            if let Some(slot) = page.allocator.allocate(want) {
                return AllocRect {
                    page: idx as u32,
                    px_x: slot.rectangle.min.x,
                    px_y: slot.rectangle.min.y,
                };
            }
        }
        // 现有页全放不下 → 开新页并在其上分配。一个字形 bitmap 永远 < 4096²
        // （rasterize_glyph 内已截断），所以新页必能容下。
        let new_idx = self.pages.len() as u32;
        self.pages
            .push(AtlasPage::new(PAGE_SIZE as u32, PAGE_SIZE as u32));
        let page = &mut self.pages[new_idx as usize];
        let slot = page
            .allocator
            .allocate(want)
            .expect("fresh 4096² page must fit any single glyph");
        AllocRect {
            page: new_idx,
            px_x: slot.rectangle.min.x,
            px_y: slot.rectangle.min.y,
        }
    }
}

/// 光栅单字形 → SDF distance（R8）+ 宽高。流程：
/// 1. 固定 `SOURCE_SIZE` 光栅（单一 SDF，所有 target size 共享）。
/// 2. 读 glyph bbox（设计单位），按 SOURCE_SIZE/units_per_em 缩放定 bitmap 尺寸。
/// 3. ttf OutlineBuilder 收集轮廓，转发到 ab_glyph_rasterizer（坐标已 scale +
///    y-flip + pad 偏移，pen 在 builder 内追踪）。
/// 4. coverage 二值化（threshold 0.5）→ 8SSEDT signed distance → R8 编码。
///
/// 像素语义：center≈128、字形内 >128、字形外 <128（distance 越大渐变越烈，超出 ±SPREAD 饱和）。
/// advance 不在此管（advance 在 layout.rs，按字体度量算）。
///
/// 无轮廓 / 无效输入走 `empty_or_tofu`：区分**真缺字**（gid=0 .notdef，字体连缺字占位都没）
/// 与**有 gid 但无轮廓的字形**（空格、零宽空格、各类 space —— 有 advance 占位无像素）。
/// 前者画 tofu 框（开发期可见占位），后者返空 bitmap 不渲染——空格被画成方块就是把
/// 这类无轮廓字形误走 tofu 的 bug。
fn rasterize_glyph(face: &Face<'_>, gid: u16) -> (Vec<u8>, u32, u32) {
    // SDF：固定 SOURCE_SIZE 光栅（单一 SDF，所有 target size 共享此源）。
    let size_px = SOURCE_SIZE;
    let units = face.units_per_em();
    if units == 0 || size_px == 0 {
        return empty_or_tofu(gid, size_px);
    }
    let scale = size_px as f32 / units as f32;
    let bbox = match face.glyph_bounding_box(GlyphId(gid)) {
        Some(b) => b,
        None => return empty_or_tofu(gid, size_px),
    };
    // bitmap = bbox(source) + 2*SPREAD（SPREAD 是 SDF spread，也是 atlas padding）。
    let gw = ((bbox.x_max - bbox.x_min) as f32 * scale).ceil() as i32 + 2 * GLYPH_PAD;
    let gh = ((bbox.y_max - bbox.y_min) as f32 * scale).ceil() as i32 + 2 * GLYPH_PAD;
    if gw <= 0 || gh <= 0 || gw > PAGE_SIZE || gh > PAGE_SIZE {
        return empty_or_tofu(gid, size_px);
    }

    // SDF 超采样：hi-res(×N) 光栅 coverage → hi-res 二值 mask → hi-res 8SSEDT →
    // 下采样 SDF 回 1x。EDT 在 hi-res 跑，zero-crossing 精度才是 hi-res；仅光栅
    // 超采样无用（ab_glyph coverage 本就是精确面积覆盖率）。
    let n = {
        let base = OVERSAMPLE.max(1) as usize;
        // 单字形 bitmap 异常大时降级，防 hi-res 光栅爆内存（正常 48pt 字 gw<200）。
        const CAP_EDGE: usize = 2048;
        if (gw as usize) * base <= CAP_EDGE && (gh as usize) * base <= CAP_EDGE {
            base
        } else {
            (CAP_EDGE / gw.max(gh) as usize).max(1).min(base)
        }
    };
    let hi_gw = gw as usize * n;
    let hi_gh = gh as usize * n;

    let mut raster = Rasterizer::new(hi_gw, hi_gh);
    let mut builder = OutlineToRaster {
        raster: &mut raster,
        // 字体设计坐标 → hi-px：scale 与 padding 都放大 n 倍（光栅在 hi-res 空间）。
        scale: scale * n as f32,
        origin_x: bbox.x_min as f32,
        // font y-up, bitmap y-down：用 y_max 做原点翻转。
        origin_y: bbox.y_max as f32,
        offset_px: GLYPH_PAD as f32 * n as f32,
        pen: Point::default(),
        start: Point::default(),
        has_contour: false,
        empty: true,
    };
    // outline_glyph 返 Some 表示字形有轮廓；这里 bbox 已 Some 故也必 Some，
    // 但仍以 builder.empty 判定真实是否有 path 被 emit（防 bbox 表与实际轮廓
    // 不一致的边缘字体）。
    let _ = face.outline_glyph(GlyphId(gid), &mut builder);
    if builder.empty {
        return empty_or_tofu(gid, size_px);
    }

    // hi-res coverage → hi-res 二值 mask（threshold 0.5）→ hi-res signed distance。
    // 距离单位 = hi-px（hi-res 像素）。
    let mut hi_mask = vec![0u8; hi_gw * hi_gh];
    raster.for_each_pixel_2d(|x, y, c| {
        let i = (y as usize) * hi_gw + (x as usize);
        hi_mask[i] = if c > 0.5 { 1 } else { 0 };
    });
    let hi_sdf = crate::text::sdf::signed_distance_field(&hi_mask, hi_gw as u32, hi_gh as u32);

    // 下采样 hi-res SDF → 1x：每像素取 n×n hi 像素距离的 box 平均（低通，平滑锯齿），
    // 再 ÷n 把距离单位从 hi-px 转回 1x px（n hi-px = 1 1x-px）。SPREAD 按 1x px 不变。
    let gw_us = gw as usize;
    let gh_us = gh as usize;
    let mut sdf = vec![0.0f32; gw_us * gh_us];
    for y in 0..gh_us {
        for x in 0..gw_us {
            let mut sum = 0.0f32;
            for sy in 0..n {
                for sx in 0..n {
                    sum += hi_sdf[(y * n + sy) * hi_gw + (x * n + sx)];
                }
            }
            sdf[y * gw_us + x] = sum / (n * n) as f32 / n as f32;
        }
    }

    let px: Vec<u8> = sdf
        .iter()
        .map(|&d| crate::text::sdf::encode_distance(d, SPREAD as u32))
        .collect();
    (px, gw as u32, gh as u32)
}

/// 无轮廓 / 无效字形兜底：gid=0（真缺字 .notdef）→ tofu 框（可见占位）；
/// gid>0 但无轮廓（空格等）→ 空 bitmap（不渲染，advance 在 layout 已算）。
fn empty_or_tofu(gid: u16, size_px: u16) -> (Vec<u8>, u32, u32) {
    if gid == 0 {
        tofu_box(size_px)
    } else {
        (Vec::new(), 0, 0)
    }
}

/// 确定性缺字 fallback：等比矩形框（上下左右 1px 边），让缺失字形可见。
/// 不算 advance（advance 在 layout.rs，由字体度量或 font_size 兜底决定）。
fn tofu_box(size_px: u16) -> (Vec<u8>, u32, u32) {
    if size_px == 0 {
        return (Vec::new(), 0, 0);
    }
    // 宽 ≈ size/2，高 ≈ size：与典型 em 方块视觉接近，但不严格等于 advance。
    let w = ((size_px as f32 / 2.0).ceil() as u32).max(2);
    let h = (size_px as u32).max(4);
    let mut px = vec![0u8; (w * h) as usize];
    // 上下边
    for x in 0..w {
        px[x as usize] = 0xdd;
        px[((h - 1) * w + x) as usize] = 0xdd;
    }
    // 左右边（h>=4 保证角不重复写有副作用，但重复写也无害）
    for y in 0..h {
        px[(y * w) as usize] = 0xdd;
        px[(y * w + w - 1) as usize] = 0xdd;
    }
    (px, w, h)
}

/// ttf-parser OutlineBuilder 实现：把字体设计坐标 scale + y-flip 后转发给
/// ab_glyph_rasterizer。Rasterizer 的 draw_line/quad/cubic 接收**绝对端点**
/// （无隐式 pen 状态），所以本 builder 自己维护 pen/start，line_to 时把
/// "pen→(x,y)" 翻成 `raster.draw_line(map(pen), map(x,y))`。
struct OutlineToRaster<'a> {
    raster: &'a mut Rasterizer,
    scale: f32,
    /// 字体设计空间 x 原点（减掉它再 scale，bbox 左边对齐 0）。
    origin_x: f32,
    /// 字体设计空间 y 原点（y_max：用 (origin_y - y)*scale 实现 y-up→y-down 翻转）。
    origin_y: f32,
    /// bitmap 内的像素 padding 偏移，让 bbox 边缘落在 padded 区。
    offset_px: f32,
    pen: Point,
    start: Point,
    has_contour: bool,
    /// 全程没 emit 任何 path → 调用方据此判 tofu。
    empty: bool,
}

impl<'a> OutlineToRaster<'a> {
    /// 字体设计坐标 → bitmap 像素坐标。
    fn map(&self, x: f32, y: f32) -> Point {
        let px = (x - self.origin_x) * self.scale + self.offset_px;
        // y 翻转：font y-up（baseline 0, ascender 正）→ bitmap y-down（顶 0）。
        let py = (self.origin_y - y) * self.scale + self.offset_px;
        point(px, py)
    }
}

impl<'a> OutlineBuilder for OutlineToRaster<'a> {
    fn move_to(&mut self, x: f32, y: f32) {
        let p = self.map(x, y);
        self.pen = p;
        self.start = p;
        self.has_contour = true;
        self.empty = false;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if !self.has_contour {
            return;
        }
        let p = self.map(x, y);
        self.raster.draw_line(self.pen, p);
        self.pen = p;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if !self.has_contour {
            return;
        }
        let c = self.map(x1, y1);
        let p = self.map(x, y);
        self.raster.draw_quad(self.pen, c, p);
        self.pen = p;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if !self.has_contour {
            return;
        }
        let c1 = self.map(x1, y1);
        let c2 = self.map(x2, y2);
        let p = self.map(x, y);
        self.raster.draw_cubic(self.pen, c1, c2, p);
        self.pen = p;
    }

    fn close(&mut self) {
        if self.has_contour && self.pen != self.start {
            self.raster.draw_line(self.pen, self.start);
            self.pen = self.start;
        }
        self.has_contour = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dejavu_face() -> Face<'static> {
        let bytes = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/DejaVuSans.ttf"
        ))
        .unwrap();
        let leaked: &'static [u8] = Box::leak(bytes.into_boxed_slice());
        Face::parse(leaked, 0).unwrap()
    }

    #[test]
    fn glyph_key_no_size_no_effect() {
        // 单一 SDF：GlyphKey 只 (font_id, glyph_id)。同字形不同 size 返同 UV（共享 SDF 槽）。
        let mut a = GlyphAtlas::new();
        let f = dejavu_face();
        let gid = f.glyph_index('A').unwrap();
        let r1 = a.ensure(
            &f,
            GlyphKey {
                font_id: 0,
                glyph_id: gid.0,
            },
        );
        let r2 = a.ensure(
            &f,
            GlyphKey {
                font_id: 0,
                glyph_id: gid.0,
            },
        );
        assert_eq!((r1.u0, r1.v0, r1.u1, r1.v1), (r2.u0, r2.v0, r2.u1, r2.v1));
    }

    #[test]
    fn ensure_then_hit_returns_same_uv() {
        let mut a = GlyphAtlas::new();
        let f = dejavu_face();
        let gid = f.glyph_index('A').unwrap();
        let k = GlyphKey {
            font_id: 0,
            glyph_id: gid.0,
        };
        let r1 = a.ensure(&f, k);
        let r2 = a.ensure(&f, k);
        assert_eq!(r1.page, r2.page);
        assert_eq!(
            (r1.u0, r1.v0, r1.u1, r1.v1),
            (r2.u0, r2.v0, r2.u1, r2.v1),
            "命中返同 UV"
        );
    }

    /// SDF 光栅：输出像素语义是 distance（中心≈128、字形内 >128、外 <128），非 coverage。
    #[test]
    fn rasterize_glyph_outputs_sdf_distance() {
        let mut a = GlyphAtlas::new();
        let f = dejavu_face();
        let gid = f.glyph_index('A').unwrap();
        let r = a.ensure(
            &f,
            GlyphKey {
                font_id: 0,
                glyph_id: gid.0,
            },
        );
        assert!(r.px_w > 0 && r.px_h > 0, "字形有轮廓");
        let (bytes, w, h) = a.page_bytes(0);
        // 找一个 inside 像素（>128）和一个 outside 像素（<128）证明 distance 语义。
        // 字形必然产生 >128（字形内）与 <128（字形外）两种值。
        let has_inside = bytes.iter().any(|&b| b > 140);
        let has_outside = bytes.iter().any(|&b| b < 100);
        assert!(has_inside, "SDF 应有 inside 像素（>140）");
        assert!(has_outside, "SDF 应有 outside 像素（<100）");
        let _ = (w, h);
    }

    #[test]
    fn new_glyph_marks_page_dirty_until_clear() {
        let mut a = GlyphAtlas::new();
        let f = dejavu_face();
        let gid = f.glyph_index('B').unwrap();
        a.ensure(
            &f,
            GlyphKey {
                font_id: 0,
                glyph_id: gid.0,
            },
        );
        assert!(!a.dirty_pages().is_empty(), "新字形标脏页");
        a.clear_dirty();
        assert!(a.dirty_pages().is_empty(), "clear 后脏页空");
        // 命中已存在字形不标脏
        a.ensure(
            &f,
            GlyphKey {
                font_id: 0,
                glyph_id: gid.0,
            },
        );
        assert!(a.dirty_pages().is_empty(), "命中不标脏");
    }

    #[test]
    fn page_bytes_nonempty_after_allocate() {
        let mut a = GlyphAtlas::new();
        let f = dejavu_face();
        let gid = f.glyph_index('C').unwrap();
        a.ensure(
            &f,
            GlyphKey {
                font_id: 0,
                glyph_id: gid.0,
            },
        );
        let (bytes, w, h) = a.page_bytes(0);
        assert!(w > 0 && h > 0, "页尺寸 > 0");
        assert_eq!(bytes.len(), (w * h) as usize, "R8 字节数 = w*h");
    }

    #[test]
    fn overflow_allocates_second_page() {
        // 用 allocate 直接填满页 0（1024² 块），然后 ensure 必须分配在页 1
        // → 证明多页溢出 + dirty 标记通路完整。
        let mut a = GlyphAtlas::new();
        // 往页 0 分 1024×1024 块直到溢出 → 页 1 被 push
        for _ in 0..50 {
            let r = a.allocate(1024, 1024);
            if r.page != 0 {
                break;
            }
        }
        assert_eq!(a.pages.len(), 2, "allocate 溢出开了页 1");
        // 清 allocate 的脏标记（allocate 不标脏，这里防御），
        // 然后 ensure 一个字形：页 0 满 → 分配在页 1。
        a.clear_dirty();
        let f = dejavu_face();
        let gid = f.glyph_index('A').unwrap();
        let k = GlyphKey {
            font_id: 0,
            glyph_id: gid.0,
        };
        let r = a.ensure(&f, k);
        assert_eq!(r.page, 1, "页 0 满 → ensure 返 page=1");
        let dirty = a.dirty_pages();
        assert!(dirty.contains(&1), "新字形所在页标脏（page=1）");
    }
}
