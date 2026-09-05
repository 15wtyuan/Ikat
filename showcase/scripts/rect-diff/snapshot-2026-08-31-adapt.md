# rect-diff 报告 — adapt 适配演示页 5 形状（2026-08-31，#110）

- 工具：browser-rect.mjs ↔ `dump_page --json`（新增 `YIO_ROOT`/`YIO_SAFE` 环境变量 +
  browser 侧 `--viewport`/`--safe` 参数，对拍「运行时 root 形状 + env() 注值」）。
- 形状：1920x1080（基线）/ 1920x1200 / 1920x1440（fit-width@4:3 类重排）/ 2560x1080
  （fit-height@21:9 类重排）/ 1920x1200+safe=60,0,40,0（env() 通道）。
- 判据口径：报告产出为主门（run-page.sh 头注契约），残余按 snapshot-2026-08-14
  四类噪声归类；结构性分歧 = unmatched（除已知 B 层注入）/ idless 错位 / 通道级差。

## 结果

| 形状 | rect diffs | unmatched | 归类 |
|---|---|---|---|
| 1920x1080 | 17 | 1（yio-anim-replay，B 层预览按钮，既有） | A 类 |
| 1920x1200 | 13 | 1（同上） | A 类 |
| 1920x1440 | 10 | 1（同上） | A 类 |
| 2560x1080 | 17 | 1（同上） | A 类 |
| 1920x1200 + safe | 13 | 1（同上） | A 类 |

全部残余 = A 类文本测量精度差（vmin 非整数字号下 harfbuzz vs ttf-parser advance
差 → 行高 1-2px → y 级联；hint 长文本 34px 宽差与 lab 页同类）。无 B/C/D 类。

## 关键正向验证（非假阴性证明）

- **vmin 字号流动双侧同步**：hero-title 高 64→85（1080→1440 形状），browser/core 同步。
- **vw 宽度流动双侧同步**：side-band 宽 230→307（1920→2560 形状），两侧一致。
- **居中锚定流动**：center-chip y 随高度平移，两侧差 ≤1px。
- **env() 通道结构对齐**：注入 safe=60,0,40,0 后 core 侧 safe-strip 高 93（33+60）
  ↔ browser 侧同值（修复 browser-rect 注值时序后；addInitScript 版本被页面加载
  序列覆盖，改为 goto 后 evaluate 注值）。

## 工具链修改

- `browser-rect.mjs`：`--viewport=WxH`（按运行时 root 开视口）+ `--safe=t,r,b,l`
  （预填 `--yio-safe-*` 变量，goto 后注值）。
- `dump_page.rs`：`YIO_ROOT`/`YIO_SAFE` 环境变量（root 形状 + safe inset）。
