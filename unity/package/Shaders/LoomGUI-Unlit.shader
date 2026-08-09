Shader "LoomGUI/Unlit"
{
    Properties
    {
        _MainTex ("Texture", 2D) = "white" {}
        _SrcFactor ("SrcFactor", Float) = 5   // SrcAlpha
        _DstFactor ("DstFactor", Float) = 10  // OneMinusSrcAlpha
        _ClipBox ("ClipBox", Vector) = (0,0,1,1)
        // _ObjectMatrix 拆 4 个 Vector 进 Properties（ShaderLab 无 Matrix property 类型）。
        // _ObjectMatrix 声明在 CBUFFER(UnityPerMaterial) 但**无 Properties 对应** → MPB.SetMatrix
        // 不覆盖非 material property 的 CBUFFER 字段 → 非 pure 节点（transform:scale/rotate）
        // _ObjectMatrix 恒默认 → 顶点塌缩到 design 原点（字消失/跑到屏幕左上）。
        // 拆 Vector 进 Properties 让 MPB.SetVector 100% 覆盖（material property），vert 内重组 float4x4。
        // 默认 4 列 = identity（pure 节点不走 OBJECT_MATRIX 路径，值无关）。
        _ObjM0 ("ObjM0", Vector) = (1,0,0,0)
        _ObjM1 ("ObjM1", Vector) = (0,1,0,0)
        _ObjM2 ("ObjM2", Vector) = (0,0,1,0)
        _ObjM3 ("ObjM3", Vector) = (0,0,0,1)
        // ColorFilter 矩阵（program=3，MPB 覆盖，同 _ObjM 模式）。
        // _CF0..3 = Matrix4x4 4 行（前 16 float），_CFOff = offset（第 5 列）。
        _CF0 ("CF0", Vector) = (1,0,0,0)
        _CF1 ("CF1", Vector) = (0,1,0,0)
        _CF2 ("CF2", Vector) = (0,0,1,0)
        _CF3 ("CF3", Vector) = (0,0,0,1)
        _CFOff ("CFOff", Vector) = (0,0,0,0)
        _Alpha ("Alpha", Float) = 1
        _CornerRadius ("CornerRadius", Float) = 0
        // Box-shadow blur（program=5 / SHADOW_BLUR）：像素空间圆角矩形 SDF + 高斯边 alpha。
        // _ShadowHalfSize.xy=像素半宽高（zw 空），_ShadowRadius=像素圆角半径，_ShadowSigma=高斯 σ（core 算），
        // _ShadowInset=0/1（inset 翻 SDF 符号做内阴影）。per-renderer MPB 覆盖。
        _ShadowHalfSize("Shadow HalfSize", Vector) = (0,0,0,0)
        _ShadowRadius("Shadow Radius", Float) = 0
        _ShadowSigma("Shadow Sigma", Float) = 0
        _ShadowInset("Shadow Inset", Float) = 0
        _FaceDilate("Face Dilate", Range(-1,1)) = 0   // 0=标准字形边缘（threshold=0.5）；正值增粗，负值变细
        _GradientScale("Gradient Scale", Float) = 13     // = SPREAD(12)+1，distance→屏幕换算（对标 TMP _GradientScale=atlasPadding+1）
        // SDF 文字效果（per-renderer MPB，program=1 ALPHA_MASK 用；参数=0 = 该 effect 不启用）。
        _OutlineWidth("Outline Width", Float) = 0
        _OutlineColor("Outline Color", Color) = (0,0,0,0)
        _UnderlayOffset0("Underlay0 Offset", Vector) = (0,0,0,0)   // xy=像素偏移
        _UnderlaySoftness0("Underlay0 Softness", Float) = 0
        _UnderlayColor0("Underlay0 Color", Color) = (0,0,0,0)
        _UnderlayOffset1("Underlay1 Offset", Vector) = (0,0,0,0)
        _UnderlaySoftness1("Underlay1 Softness", Float) = 0
        _UnderlayColor1("Underlay1 Color", Color) = (0,0,0,0)
        _UnderlayOffset2("Underlay2 Offset", Vector) = (0,0,0,0)
        _UnderlaySoftness2("Underlay2 Softness", Float) = 0
        _UnderlayColor2("Underlay2 Color", Color) = (0,0,0,0)
        _GlowPower("Glow Power", Float) = 0
        _GlowColor("Glow Color", Color) = (0,0,0,0)
        _BlurWidth("Blur Width", Float) = 0
    }
    SubShader
    {
        Tags { "RenderPipeline" = "UniversalPipeline" "Queue" = "Transparent" "RenderType" = "Transparent" }
        Cull Off
        ZWrite Off
        Blend [_SrcFactor] [_DstFactor]

        Pass
        {
            HLSLPROGRAM
            #pragma vertex vert
            #pragma fragment frag
            #pragma multi_compile _ CLIPPED
            // CLIPPED_ROUNDED（祖先 overflow clip 圆角遮罩）与 SHADOW_BLUR（box-shadow 自身 SDF 圆角 + 高斯边）
            // 拆独立 multi_compile 行：同 Material 可同时启用（blur shadow 落在 rounded-overflow 容器内），
            // 同行时 Unity 只选首个声明变体（CLIPPED_ROUNDED 赢）→ SHADOW_BLUR 块不执行 → 阴影塌成硬裁剪块。
            // 两块独立（各自 col.a *= …），同时启用正确叠加。shadow 自身圆角走 SDF，但仍经 mask_context
            // 受祖先 overflow clip 约束（非“无需 clip”）。
            #pragma multi_compile _ CLIPPED_ROUNDED
            #pragma multi_compile _ SHADOW_BLUR
            #pragma multi_compile _ OBJECT_MATRIX
            #pragma multi_compile _ ALPHA_MASK
            #pragma multi_compile _ BG_COMPOSITE
            #pragma multi_compile _ COLOR_FILTER
            #include "Packages/com.unity.render-pipelines.universal/ShaderLibrary/Core.hlsl"

            struct Attr { float4 pos : POSITION; float4 color : COLOR; float2 uv : TEXCOORD0; };
            struct Vary { float4 pos : SV_POSITION; float4 color : COLOR; float2 uv : TEXCOORD0;
                          float2 clipPos : TEXCOORD1; };

            CBUFFER_START(UnityPerMaterial)
                float4 _MainTex_ST;
                float4 _ClipBox;
                // _ObjectMatrix 拆 4 Vector（Properties 对应，MPB 覆盖）。列主序：重组 float4x4(_ObjM0..3)。
                float4 _ObjM0;
                float4 _ObjM1;
                float4 _ObjM2;
                float4 _ObjM3;
                float4 _CF0;
                float4 _CF1;
                float4 _CF2;
                float4 _CF3;
                float4 _CFOff;
                float _Alpha;
                float _CornerRadius;   // 归一化圆角半径（design_radius / min_half_size），CLIPPED_ROUNDED 用
                // Box-shadow blur uniforms（Properties 对应，SRP batcher 须入 CBUFFER；per-renderer MPB 覆盖）。
                float4 _ShadowHalfSize;
                float _ShadowRadius;
                float _ShadowSigma;
                float _ShadowInset;
                float _FaceDilate;
                float _GradientScale;
                // SDF 文字效果 uniforms（Properties 对应，MPB per-renderer 覆盖；参数=0 = 该 effect 不启用）。
                float _OutlineWidth;
                half4 _OutlineColor;
                float4 _UnderlayOffset0; float _UnderlaySoftness0; half4 _UnderlayColor0;
                float4 _UnderlayOffset1; float _UnderlaySoftness1; half4 _UnderlayColor1;
                float4 _UnderlayOffset2; float _UnderlaySoftness2; half4 _UnderlayColor2;
                float _GlowPower;
                half4 _GlowColor;
                float _BlurWidth;
            CBUFFER_END
            TEXTURE2D(_MainTex); SAMPLER(sampler_MainTex);
            // Unity 按 {TextureName}_TexelSize 名字约定自动填充（.xy=1/wh, .zw=wh）；须显式声明才能在 HLSL 引用。
            // 不放 UnityPerMaterial CBUFFER：引擎按纹理（非 material）填充，SRP batcher 不要求入 CBUFFER。
            float4 _MainTex_TexelSize;

            Vary vert(Attr v) {
                Vary o;
                // 两路径统一经 TransformObjectToWorld：GO 是 root 子 → ObjectToWorld = root_ObjectToWorld
                // （把 design world → Unity world，含 sf 缩放 + y-flip + rootPos）。
#if defined(OBJECT_MATRIX)
                // _ObjectMatrix（4 Vector 重组）把 box-local 顶点 → design world；再 TransformObjectToWorld → Unity world。
                // 直接 TransformWorldToHClip(designWorld) 漏 root transform（design 坐标 ≠ Unity world），
                // 非纯平移节点会位置/翻转/缩放全错，且与命中（design world matrix 逆投）不一致 → 点不到。
                float4x4 objM = float4x4(_ObjM0, _ObjM1, _ObjM2, _ObjM3);
                float3 designWorld = mul(objM, float4(v.pos.xy, 0, 1)).xyz;
                float3 worldPos = TransformObjectToWorld(designWorld);
#else
                float3 worldPos = TransformObjectToWorld(v.pos.xyz);
#endif
                o.pos = TransformWorldToHClip(worldPos);
                float2 clipWorldXY = worldPos.xy;
                o.color = v.color;
                // SHADOW_BLUR：core 把 uv 编码为「顶点本地坐标 − 形状中心」（像素量纲，无纹理），
                // 须直通 raw uv；TRANSFORM_TEX 会叠 _MainTex_ST 缩放/偏移 → SDF 坐标错位（静默依赖 _MainTex_ST==1,1,0,0）。
#if defined(SHADOW_BLUR)
                o.uv = v.uv;
#else
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
#endif
#if defined(CLIPPED) || defined(CLIPPED_ROUNDED)
                o.clipPos = clipWorldXY * _ClipBox.zw + _ClipBox.xy;
#endif
                return o;
            }
            half4 frag(Vary i) : SV_Target {
                // vertex color 来自 CSS（sRGB 编码）；Linear 项目 Unity 不自动转 vertex color → 须手动 sRGB→linear，
                // 否则颜色偏浅/灰蒙蒙（#1a1d2e sRGB 0.10 当 linear 显示 ~0.35）。texture 是 sRGB format 自动转，不重复。alpha 线性不转。
                half4 vcol = i.color;
                // sRGB → linear（精确 sRGB 公式；CSS 颜色 sRGB，Linear 项目 Unity 不自动转 vertex color）。
                half3 sc = vcol.rgb;
                vcol.rgb = (sc <= 0.04045) ? sc / 12.92 : pow((sc + 0.055) / 1.055, 2.4);
                half4 tex = SAMPLE_TEXTURE2D(_MainTex, sampler_MainTex, i.uv);
                #if defined(ALPHA_MASK)
                // SDF：tex.r 是 encoded distance（中心 0.5、inside>0.5）。
                float2 uvDx = ddx(i.uv);
                float2 uvDy = ddy(i.uv);
                float pxSize = rsqrt(abs(uvDx.x * uvDy.y - uvDx.y * uvDy.x));
                float scale = pxSize * (1.3333 * _GradientScale) / _MainTex_TexelSize.z;
                float d = tex.r;
                // 字形边缘有符号距离（screen-px 量纲，+=内侧、-=外侧）。所有 effect 宽度与之
                // 同量纲——避免 mask 塌成常数把整个 quad 铺成半透方块。_FaceDilate 正值外推边缘（增粗）。
                float edge = (d - 0.5) * scale + _FaceDilate * 0.5 * scale;

                // FACE：blur>0 时按 _BlurWidth 软化过渡带（SDF 近似整字高斯 blur，偏硬，验收接受）。
                // blur 旧式 `scale/=1+blur*scale` 会把 scale 压到 ~0.5 → face≈0.5 铺满 quad（方块底）；
                // 改为 edge/blur 做 transition 宽度，blur=0 时退化回 1px AA。
                float faceSoft = max(_BlurWidth, 1.0);
                float face = saturate(edge / faceSoft + 0.5);

                float3 rgb = vcol.rgb;
                float a = face * vcol.a;
                // underlay×3（shadow：偏移重采 + softness=blur 软化，over 合成画 face 下）。
                // CSS ox=右、oy=下；用屏幕导数把像素偏移转 UV（自动适配 y-flip/缩放，量纲正确）；
                // shadow 应落 +offset 方向 → 在 i.uv - offUv 处采样（旧代码 + 号致方向反到左上）。
                #define UNDERLAY_PASS(idx) \
                    if (_UnderlayColor##idx.a > 0.001) { \
                        float2 offUv = _UnderlayOffset##idx.x * uvDx + _UnderlayOffset##idx.y * uvDy; \
                        float dd = SAMPLE_TEXTURE2D(_MainTex, sampler_MainTex, i.uv - offUv).r; \
                        float de = (dd - 0.5) * scale + _FaceDilate * 0.5 * scale; \
                        float usoft = max(_UnderlaySoftness##idx, 1.0); \
                        float um = saturate(de / usoft + 0.5); \
                        float ua = _UnderlayColor##idx.a * um; \
                        rgb = lerp(rgb, _UnderlayColor##idx.rgb, ua * (1.0 - a)); \
                        a += ua * (1.0 - a); \
                    }
                UNDERLAY_PASS(0)
                UNDERLAY_PASS(1)
                UNDERLAY_PASS(2)
                #undef UNDERLAY_PASS
                // outline（stroke：edge 外侧 halfW 宽环，over 合成画 face 下——face 覆盖内侧半环，
                // 外侧半环露出描边色）。旧代码把 _OutlineWidth(px) 直接加进 d（1 d 单位≈24px）→
                // outer/inner 双 saturate → 整个 glyph 填描边色盖掉字色，看不出描边。
                if (_OutlineWidth > 0.001) {
                    float halfW = _OutlineWidth * 0.5;
                    float om = saturate(edge + halfW + 0.5) - saturate(edge + 0.5);
                    float oa = om * _OutlineColor.a;
                    rgb = lerp(rgb, _OutlineColor.rgb, oa * (1.0 - a));
                    a += oa * (1.0 - a);
                }
                // glow（edge 外晕开，_GlowPower=晕开半径 px，曲线衰减）。旧 `gm=1-face` 远离字形处
                // face→0 → gm→1 → glow 满 quad；改 outDist 仅取外侧距离，到 glowExt 衰减到 0。
                if (_GlowColor.a > 0.001) {
                    float outDist = max(-edge, 0.0);
                    float glowExt = max(_GlowPower, 1.0);
                    float gm = pow(saturate(1.0 - outDist / glowExt), 2.0);
                    float ga = gm * _GlowColor.a;
                    rgb = lerp(rgb, _GlowColor.rgb, ga * (1.0 - a));
                    a += ga * (1.0 - a);
                }
                half4 col = half4(rgb, a);
                #elif defined(BG_COMPOSITE)
                // Container+bg-image（program:2/4）：CSS background 合成 = 图(tex) over 底色(vcol)，结果直通配合 SrcAlpha blend。
                // 旧 col.a=vcol.a：无 bg-color(vcol.a=0)时全透明丢图（验收 §3.6第4/§3.7/§3.9 图消失）。
                // 标准 source-over：a=tex.a+vcol.a·(1−tex.a)；rgb 直通=预乘/a（max 防除零；a=0 像素 Blend 不贡献，rgb 无关）。
                // 有底色不透明(vcol.a=1)：a=1, rgb=图叠底色（与旧公式完全一致，零回归）。
                // 无底色(vcol.a=0)：a=tex.a, rgb=tex.rgb（等价 program:0 图直通，图显透明区透下层）。
                float bgA = tex.a + vcol.a * (1.0 - tex.a);
                float3 bgRgb = ((float3)tex.rgb * tex.a + (float3)vcol.rgb * vcol.a * (1.0 - tex.a)) / max(bgA, 1e-6);
                half4 col = half4(bgRgb, bgA);
                #else
                // image/mesh（program:0）：彩色 texture → tex.rgb × vcol。
                half4 col = tex * vcol;
                #endif
                #if defined(COLOR_FILTER)
                // CSS filter 定义在 sRGB 空间（矩阵 offset 如 contrast -0.25 = sRGB 中点 0.5 的偏移）。
                // col.rgb 当前 linear → linear→sRGB → 矩阵 → sRGB→linear，中点/色相才与浏览器对齐。
                // max(.,0) 防 pow 负底数 NaN（矩阵可出负值或超 1，最终 Blend 输出时再裁）。cfs 避免与上方 sc 重名。
                half3 cfs = col.rgb;
                cfs = (cfs <= 0.0031308) ? cfs * 12.92 : 1.055 * pow(max(cfs, 0.0), 1.0 / 2.4) - 0.055;
                float4x4 cfM = float4x4(_CF0, _CF1, _CF2, _CF3);
                cfs = mul(cfM, float4(cfs, 1.0)).rgb + _CFOff.rgb;
                cfs = (cfs <= 0.04045) ? cfs / 12.92 : pow(max((cfs + 0.055) / 1.055, 0.0), 2.4);
                col.rgb = cfs;
                #endif
                // 节点 opacity（从顶点色剥离，per-renderer MPB）。alpha 剥离后 colors.a 不含节点 alpha。
                col.a *= _Alpha;
                #ifdef CLIPPED_ROUNDED
                // 圆角矩形 SDF 裁剪：clipPos 在 _ClipBox 归一化空间（|x|,|y|<=1 在直角矩形内）。
                // rounded-box SDF：q = abs(p) - half + r（half=1 归一化），sdf = length(max(q,0)) + min(max(q.x,q.y),0) - r。
                // sdf<0 内，>0 外。smoothstep 抗锯齿（1px 过渡带，与 clipPos 归一化空间分辨率匹配）。
                float r = _CornerRadius;
                float2 q = abs(i.clipPos) - 1.0 + r;
                float sdf = length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
                col.a *= 1.0 - smoothstep(0.0, 1.0, sdf);
                #elif defined(CLIPPED)
                float2 f = abs(i.clipPos);
                col.a *= step(max(f.x, f.y), 1.0);
                #endif
                #ifdef SHADOW_BLUR
                // Box-shadow blur（program=5）：像素空间圆角矩形 SDF + 高斯边 alpha 衰减。
                // i.uv 由 core 几何编码为「顶点本地坐标 − 形状中心」，量纲=像素；故 _ShadowHalfSize.xy/_ShadowRadius
                // 亦取像素，SDF 公式同 CLIPPED_ROUNDED（半宽/半径改像素量纲，归一化半宽 1 换成 _ShadowHalfSize.xy）。
                // σ=_ShadowSigma（core 从 CSS blur 算好，shader 不重算）；inset 翻 SDF 符号 → 取内侧衰减做内阴影。
                // exp(-d²/2σ²) 而非 erfc（HLSL 无 erfc 内建）；max(d,0) 只衰减外（outset）/内侧（inset）半边。
                float2 p = i.uv;
                float qx = abs(p.x) - _ShadowHalfSize.x + _ShadowRadius;
                float qy = abs(p.y) - _ShadowHalfSize.y + _ShadowRadius;
                // 命名 shadowSdf（非 sdf）：CLIPPED_ROUNDED 块也声明 sdf，I1 路径（shadow 在圆角
                // overflow 容器）两 keyword 共启 → 同名 redefinition 编译错。两块各用专名避撞。
                float shadowSdf = length(max(float2(qx, qy), 0.0)) + min(max(qx, qy), 0.0) - _ShadowRadius;
                float d = (_ShadowInset > 0.5) ? -shadowSdf : shadowSdf;
                float sig = max(_ShadowSigma, 0.0001);
                float g = max(d, 0.0);
                col.a *= exp(-(g * g) / (2.0 * sig * sig));
                #endif
                return col;
            }
            ENDHLSL
        }
    }
}
