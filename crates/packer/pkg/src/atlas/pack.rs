//! shelf 打包：SourceImage 列表 → atlas 页（RgbaImage）+ AtlasManifest。
//! 复用 etagere（core 字体图集同款）。禁旋转/trim（轴对齐，对齐 Unity 侧包装约束）。

use super::collect::SourceImage;
use super::{AtlasManifest, SpriteEntry};
use crate::workspace::AtlasCfg;
use etagere::{size2, AtlasAllocator};
use std::collections::BTreeMap;

/// 打包结果：清单 + 每页像素（caller 负责编码写盘）。
pub struct PackedAtlas {
    pub manifest: AtlasManifest,
    pub pages: Vec<image::RgbaImage>,
}

/// 把一个图集的源图 shelf 打包成若干页。
pub fn pack_atlas(atlas: &AtlasCfg, images: &[SourceImage]) -> Result<PackedAtlas, String> {
    let pad = atlas.padding as i32;
    let max = atlas.max_size;

    let mut pages: Vec<image::RgbaImage> = Vec::new();
    let mut allocators: Vec<AtlasAllocator> = Vec::new();
    let mut sprites: BTreeMap<String, SpriteEntry> = BTreeMap::new();

    for img in images {
        // 单图 + padding 比页还大 → 明确报错（AI 可诉诸行动：调大 max_size 或用 standalone）。
        let need_w = img.w as i32 + pad * 2;
        let need_h = img.h as i32 + pad * 2;
        if need_w > max as i32 || need_h > max as i32 {
            return Err(format!(
                "图 `{}`（{}×{} + padding {}）超过图集 `{}` 单页上限 {}；调大 max_size 或改 standalone",
                img.key, img.w, img.h, atlas.padding, atlas.name, max
            ));
        }

        // standalone：每图独立成页；否则尝试塞进已有页，失败再开新页。
        let (page_idx, alloc) = if atlas.standalone {
            new_page(&mut pages, &mut allocators, max);
            let idx = pages.len() - 1;
            let a = allocators[idx]
                .allocate(size2(need_w, need_h))
                .ok_or_else(|| format!("standalone 分配失败：{}", img.key))?;
            (idx, a)
        } else {
            let mut placed: Option<(usize, etagere::Allocation)> = None;
            for (idx, allocator) in allocators.iter_mut().enumerate() {
                if let Some(a) = allocator.allocate(size2(need_w, need_h)) {
                    placed = Some((idx, a));
                    break;
                }
            }
            match placed {
                Some(p) => p,
                None => {
                    new_page(&mut pages, &mut allocators, max);
                    let idx = pages.len() - 1;
                    let a = allocators[idx]
                        .allocate(size2(need_w, need_h))
                        .ok_or_else(|| format!("新页分配失败：{}", img.key))?;
                    (idx, a)
                }
            }
        };

        // blit 到页（跳过 padding 边）。
        let x0 = alloc.rectangle.min.x + pad;
        let y0 = alloc.rectangle.min.y + pad;
        blit(&mut pages[page_idx], &img.rgba, x0 as u32, y0 as u32);

        // 归一化 UV。
        let pw = max as f32;
        let ph = max as f32;
        let u0 = x0 as f32 / pw;
        let v0 = y0 as f32 / ph;
        let u1 = (x0 as u32 + img.w) as f32 / pw;
        let v1 = (y0 as u32 + img.h) as f32 / ph;
        sprites.insert(
            img.key.clone(),
            SpriteEntry {
                page: page_idx as u32,
                uv: [u0, v0, u1, v1],
                orig: [img.w, img.h],
            },
        );
    }

    let page_names: Vec<String> = (0..pages.len())
        .map(|i| page_file_name(&atlas.name, i))
        .collect();

    Ok(PackedAtlas {
        manifest: AtlasManifest {
            pages: page_names,
            sprites,
        },
        pages,
    })
}

/// 页文件名：第 0 页 `<name>.png`，其后 `<name>.<n>.png`。
pub fn page_file_name(atlas_name: &str, idx: usize) -> String {
    if idx == 0 {
        format!("{atlas_name}.png")
    } else {
        format!("{atlas_name}.{idx}.png")
    }
}

fn new_page(pages: &mut Vec<image::RgbaImage>, allocators: &mut Vec<AtlasAllocator>, max: u32) {
    pages.push(image::RgbaImage::from_pixel(
        max,
        max,
        image::Rgba([0, 0, 0, 0]),
    ));
    allocators.push(AtlasAllocator::new(size2(max as i32, max as i32)));
}

fn blit(page: &mut image::RgbaImage, src: &image::RgbaImage, x0: u32, y0: u32) {
    for y in 0..src.height() {
        for x in 0..src.width() {
            page.put_pixel(x0 + x, y0 + y, *src.get_pixel(x, y));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn img(key: &str, w: u32, h: u32) -> SourceImage {
        SourceImage {
            key: key.into(),
            rgba: image::RgbaImage::from_pixel(w, h, image::Rgba([1, 2, 3, 255])),
            w,
            h,
        }
    }

    fn cfg(max: u32, standalone: bool) -> AtlasCfg {
        AtlasCfg {
            name: "ui".into(),
            standalone,
            dirs: vec![],
            max_size: max,
            padding: 2,
        }
    }

    #[test]
    fn packs_all_into_one_page_no_overlap() {
        let images = vec![
            img("a.png", 16, 16),
            img("b.png", 16, 16),
            img("c.png", 16, 16),
        ];
        let packed = pack_atlas(&cfg(256, false), &images).unwrap();
        assert_eq!(packed.pages.len(), 1);
        // 覆盖：每个输入都有条目。
        assert_eq!(packed.manifest.sprites.len(), 3);
        // 不重叠：任意两 sprite 的像素 rect（由 uv×256 反推）不相交。
        let rects: Vec<[u32; 4]> = packed
            .manifest
            .sprites
            .values()
            .map(|s| {
                [
                    (s.uv[0] * 256.0).round() as u32,
                    (s.uv[1] * 256.0).round() as u32,
                    (s.uv[2] * 256.0).round() as u32,
                    (s.uv[3] * 256.0).round() as u32,
                ]
            })
            .collect();
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let a = rects[i];
                let b = rects[j];
                let disjoint = a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1];
                assert!(disjoint, "sprite {i} 与 {j} 重叠：{a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn overflow_opens_new_page() {
        // max=64, padding=2 → 每张 58x58+pad 只能塞一张/页 → 3 张 = 3 页。
        let images = vec![
            img("a.png", 58, 58),
            img("b.png", 58, 58),
            img("c.png", 58, 58),
        ];
        let packed = pack_atlas(&cfg(64, false), &images).unwrap();
        assert!(
            packed.pages.len() >= 2,
            "溢出应多页，实际 {}",
            packed.pages.len()
        );
        assert_eq!(packed.manifest.pages[0], "ui.png");
        assert_eq!(packed.manifest.pages[1], "ui.1.png");
    }

    #[test]
    fn standalone_one_per_page() {
        let images = vec![img("a.png", 16, 16), img("b.png", 16, 16)];
        let packed = pack_atlas(&cfg(256, true), &images).unwrap();
        assert_eq!(packed.pages.len(), 2, "standalone 每图独立成页");
    }

    #[test]
    fn oversized_single_image_errors() {
        let images = vec![img("huge.png", 300, 10)];
        let err = pack_atlas(&cfg(256, false), &images);
        assert!(err.is_err(), "单图超页应报错");
    }
}
