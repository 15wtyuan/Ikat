//! 8SSEDT（8-point signed sequential Euclidean distance transform）：二值字形 mask
//! → 每像素到边缘的 signed distance（inside 正 / outside 负）。
//! 算法 = Felzenszwalb & Huttenlocher 二维 EDT（下包络线 1D EDT，先行再列两遍）。
//! signed = sqrt(edt_seed_outside) - sqrt(edt_seed_inside)：
//!   inside 像素 edt_inside=0 → signed = +到最近 outside 的距离；
//!   outside 像素 edt_outside=0 → signed = -到最近 inside 的距离。

/// 二值 mask（inside=1/outside=0）→ 每像素 signed distance（像素单位）。
/// mask 长度须 == (w*h)；越界/空返回空 Vec（FFI 邻近不 panic）。
pub fn signed_distance_field(mask: &[u8], w: u32, h: u32) -> Vec<f32> {
    let n = (w as usize) * (h as usize);
    if w == 0 || h == 0 || mask.len() != n {
        return Vec::new();
    }
    let w = w as usize;
    let h = h as usize;
    // 两个 seed grid：outside-seed（求 inside 像素到边缘）、inside-seed（求 outside 像素到边缘）。
    let mut f_outside = vec![f32::INFINITY; n]; // seed = inside 像素（0）
    let mut f_inside = vec![f32::INFINITY; n]; //  seed = outside 像素（0）
    for i in 0..n {
        if mask[i] != 0 {
            f_outside[i] = 0.0; // inside 是 outside-distance 的 seed
        } else {
            f_inside[i] = 0.0; // outside 是 inside-distance 的 seed
        }
    }
    edt_2d(&mut f_outside, w, h);
    edt_2d(&mut f_inside, w, h);
    let mut out = vec![0.0f32; n];
    for i in 0..n {
        let d_out = f_outside[i].max(0.0).sqrt(); // 到最近 inside 的距离
        let d_in = f_inside[i].max(0.0).sqrt(); //  到最近 outside 的距离
                                                // signed：inside 像素（mask=1）→ +d_in（到边缘外侧）；outside → -d_out。
        out[i] = if mask[i] != 0 { d_in } else { -d_out };
    }
    out
}

/// Felzenszwalb 1D EDT：f[i] = 该点已知的平方距离（seed=0，余 INF）→ out[i] = min_j(f[j]+(i-j)²)。
fn edt_1d(f: &[f32], out: &mut [f32]) {
    let n = f.len();
    if n == 0 {
        return;
    }
    let mut v = vec![0usize; n]; // 下包络顶点索引
    let mut z = vec![f32::NEG_INFINITY; n + 1]; // 拐点
    z[0] = f32::NEG_INFINITY;
    z[1] = f32::INFINITY;
    let mut k = 0usize;
    for q in 1..n {
        if f[q].is_infinite() {
            continue; // 非 seed 不参与包络
        }
        // 求 parabola(q) 与当前包络交点，弹掉被完全覆盖的旧顶点。
        let mut s = intersec(f, v[k], q);
        while s <= z[k] {
            if k == 0 {
                break;
            }
            k -= 1;
            s = intersec(f, v[k], q);
        }
        k += 1;
        v[k] = q;
        z[k] = s;
        z[k + 1] = f32::INFINITY;
    }
    let mut k = 0usize;
    #[allow(clippy::needless_range_loop)]
    for q in 0..n {
        while z[k + 1] < q as f32 {
            k += 1;
        }
        let dd = q as f32 - v[k] as f32;
        out[q] = dd * dd + f[v[k]];
    }
}

/// 两 parabola 交点横坐标（seed r 与 seed q）。
fn intersec(f: &[f32], r: usize, q: usize) -> f32 {
    let fr = f[r];
    let fq = f[q];
    ((fq + (q * q) as f32) - (fr + (r * r) as f32)) / (2.0 * (q - r) as f32)
}

/// 二维 EDT：先每行 1D，再每列 1D（对行结果）。结果为平方距离。
fn edt_2d(f: &mut [f32], w: usize, h: usize) {
    let mut tmp = vec![0.0f32; w];
    for y in 0..h {
        let row = &mut f[y * w..y * w + w];
        edt_1d(row, &mut tmp);
        row.copy_from_slice(&tmp);
    }
    let mut col_in = vec![0.0f32; h];
    let mut col_out = vec![0.0f32; h];
    for x in 0..w {
        for y in 0..h {
            col_in[y] = f[y * w + x];
        }
        edt_1d(&col_in, &mut col_out);
        for y in 0..h {
            f[y * w + x] = col_out[y];
        }
    }
}

/// signed distance（像素）→ R8 编码：中心 0.5，inside>0.5、outside<0.5。
/// 超出 ±spread 的 distance 饱和到 0/255（spread 之外 effect 会被 clip，SDF 硬约束）。
pub fn encode_distance(d: f32, spread: u32) -> u8 {
    let spread = spread.max(1) as f32;
    let normalized = 0.5 + d / (2.0 * spread);
    (normalized.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// R8 → signed distance（像素），encode_distance 的逆。
pub fn decode_distance(u: u8, spread: u32) -> f32 {
    let spread = spread.max(1) as f32;
    (u as f32 / 255.0 - 0.5) * 2.0 * spread
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 实心矩形 mask（去掉 1px 边框）→ 内部距边缘应全部 >0。
    #[test]
    fn inside_pixels_positive_outside_negative() {
        // 5×5，中心 3×3 实心 inside（去掉外圈）。
        let mut mask = vec![0u8; 25];
        for y in 1..4 {
            for x in 1..4 {
                mask[y * 5 + x] = 1;
            }
        }
        let sdf = signed_distance_field(&mask, 5, 5);
        // 中心 (2,2) 在 inside，距最近 outside=2px → signed≈+2。
        assert!(sdf[2 * 5 + 2] > 1.5, "中心 inside 距离应 ≈2");
        // 角 (0,0) 在 outside，距最近 inside≈1px → signed≈-1.4。
        assert!(sdf[0] < -0.5, "角 outside 距离应为负");
    }

    /// 边缘 zero-crossing：跨越字形轮廓时 signed 从正变负。
    #[test]
    fn edge_zero_crossing() {
        // 左半 inside、右半 outside（竖直边缘在 x=2.5）。
        let mut mask = vec![0u8; 10]; // 5×2
        for y in 0..2 {
            for x in 0..2 {
                mask[y * 5 + x] = 1;
            }
        }
        let sdf = signed_distance_field(&mask, 5, 2);
        // x=1（inside 边缘）正，x=2（紧贴边缘 outside）负或小正。
        assert!(sdf[1] >= 0.0, "inside 边缘非负");
        assert!(sdf[3] <= 0.0, "outside 为负");
    }

    /// encode/decode 往返误差 < 1px/spread。
    #[test]
    fn encode_decode_roundtrip() {
        let spread = 12u32;
        for d in [-11.5f32, -6.0, -1.0, 0.0, 0.5, 5.0, 11.5] {
            let enc = encode_distance(d, spread);
            let dec = decode_distance(enc, spread);
            assert!((dec - d).abs() < 1.0, "d={d} 往返 dec={dec} 误差过大");
        }
        // 超 spread 饱和：encode 钳到 [0,255]，decode 回到 ∓spread。
        assert_eq!(encode_distance(20.0, 12), 255);
        assert_eq!(encode_distance(-20.0, 12), 0);
    }

    /// 空输入不 panic（FFI 邻近容错）。
    #[test]
    fn empty_input_no_panic() {
        let s = signed_distance_field(&[], 0, 0);
        assert!(s.is_empty());
    }
}
