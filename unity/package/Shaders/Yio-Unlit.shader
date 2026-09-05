Shader "Yio/Unlit"
{
    Properties
    {
        _MainTex ("Texture", 2D) = "white" {}
        _SrcFactor ("SrcFactor", Float) = 5   // SrcAlpha
        _DstFactor ("DstFactor", Float) = 10  // OneMinusSrcAlpha
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
        // 节点 design 平移（Mtx,Mty，per-renderer MPB 覆盖）：clip 链测试空间是 design
        // 坐标，而 blob 顶点已 re-base 到节点本地（纯平移行）/盒本地（OBJECT_MATRIX 行），
        // 须补回平移（合并行 Mtx=Mty=0，顶点已是绝对 design——同式自洽）。
        _ObjT ("ObjT", Vector) = (0,0,0,0)
        // Box-shadow blur（program=5 / SHADOW_BLUR）：像素空间圆角矩形 SDF + 高斯边 alpha。
        // _ShadowHalfSize.xy=像素半宽高（zw 空），_ShadowRadius=像素圆角半径，_ShadowSigma=高斯 σ（core 算），
        // _ShadowInset=0/1（inset 翻 SDF 符号做内阴影）。per-renderer MPB 覆盖。
        _ShadowHalfSize("Shadow HalfSize", Vector) = (0,0,0,0)
        _ShadowRadius("Shadow Radius", Float) = 0
        _ShadowSigma("Shadow Sigma", Float) = 0
        _ShadowInset("Shadow Inset", Float) = 0
        // 背景渐变（program=6/7 GRADIENT，per-renderer MPB 覆盖；未用 stop 槽由 C# 填
        // 「末 stop 色 @pos=1」→ shader 无需 count uniform，8 槽段搜索自然退化到末 stop）。
        // _GradGeom = linear（dir.xy, t0, inv_span）；_GradGeom2 = radial（center.xy, radii.xy）。
        _GradKind("Grad Kind", Float) = 0
        _GradGeom("Grad Geom", Vector) = (1,0,0,1)
        _GradGeom2("Grad Geom 2", Vector) = (0,0,1,1)
        _GradStop0("Grad Stop 0", Vector) = (1,1,1,1)
        _GradPos0("Grad Pos 0", Float) = 0
        _GradStop1("Grad Stop 1", Vector) = (1,1,1,1)
        _GradPos1("Grad Pos 1", Float) = 1
        _GradStop2("Grad Stop 2", Vector) = (1,1,1,1)
        _GradPos2("Grad Pos 2", Float) = 1
        _GradStop3("Grad Stop 3", Vector) = (1,1,1,1)
        _GradPos3("Grad Pos 3", Float) = 1
        _GradStop4("Grad Stop 4", Vector) = (1,1,1,1)
        _GradPos4("Grad Pos 4", Float) = 1
        _GradStop5("Grad Stop 5", Vector) = (1,1,1,1)
        _GradPos5("Grad Pos 5", Float) = 1
        _GradStop6("Grad Stop 6", Vector) = (1,1,1,1)
        _GradPos6("Grad Pos 6", Float) = 1
        _GradStop7("Grad Stop 7", Vector) = (1,1,1,1)
        _GradPos7("Grad Pos 7", Float) = 1
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
            // CLIPPED（#52 多 entry clip 链）：rect / 圆角 SDF / circle / polygon 按 entry
            // kind 分派（_ClipFrame0[e].w），链上全部 entry 逐条测试全过才保留（web clip 栈
            // 交集语义）。与 SHADOW_BLUR 独立两行 multi_compile（同时启用正确叠加——两块
            // 各自 col.a *= …；shadow 自身圆角走 SDF，但仍经 mask_context 受祖先裁剪约束）。
            #pragma multi_compile _ CLIPPED
            #pragma multi_compile _ SHADOW_BLUR
            #pragma multi_compile _ OBJECT_MATRIX
            #pragma multi_compile _ ALPHA_MASK
            #pragma multi_compile _ BG_COMPOSITE
            #pragma multi_compile _ COLOR_FILTER
            // 背景渐变（program=6/7）：kind 走 uniform 分支（per-draw 一致，无 GPU 代价），
            // 单变体避免与 COLOR_FILTER/CLIPPED 组合爆炸。
            #pragma multi_compile _ GRADIENT
            #include "Packages/com.unity.render-pipelines.universal/ShaderLibrary/Core.hlsl"

            struct Attr { float4 pos : POSITION; float4 color : COLOR; float2 uv : TEXCOORD0; };
            struct Vary { float4 pos : SV_POSITION; float4 color : COLOR; float2 uv : TEXCOORD0;
                          float2 designPos : TEXCOORD1; };

            CBUFFER_START(UnityPerMaterial)
                float4 _MainTex_ST;
                // clip 链（#52，CLIPPED 变体；MaterialManager.SetClipEntries 经
                // SetVectorArray 写入——数组不可作 ShaderLab Properties，只能在 CBUFFER
                // 声明由 material API 覆盖）。4 entry 定长 = core MAX_CLIP_CHAIN。
                // frame0 = (A, C, Tx, kind)，frame1 = (B, D, Ty, hasRect)：
                // lp = (A,C)·designPos + Tx 等六元组逆矩阵映 clipper box-local。
                // kind：0=rect 1=rounded 2=circle 3=polygon。
                float4 _ClipFrame0[4];
                float4 _ClipFrame1[4];
                float4 _ClipRect[4];     // (w, h, poly_count, _)
                float4 _ClipRadii0[4];   // (tl_rx, tl_ry, tr_rx, tr_ry)
                float4 _ClipRadii1[4];   // (br_rx, br_ry, bl_rx, bl_ry)
                float4 _ClipCircle[4];   // (cx, cy, r, _)
                float4 _ClipPoly[32];    // 每 entry 8 个 float4 × 2 点（16 点上限）
                float _ClipCount;
                // 哑字段：MaterialManager 每次 clip 链更新递增写入——值恒变逼 SRP batcher
                // 重建材质 CBUFFER（对「数组同长重写」不失效的保险）。shader 逻辑不用。
                float _ClipGen;
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
                float4 _ObjT;
                // Box-shadow blur uniforms（Properties 对应，SRP batcher 须入 CBUFFER；per-renderer MPB 覆盖）。
                float4 _ShadowHalfSize;
                float _ShadowRadius;
                float _ShadowSigma;
                float _ShadowInset;
                // 背景渐变 uniforms（Properties 对应，MPB per-renderer 覆盖；SRP batcher 须入 CBUFFER）。
                float _GradKind;
                float4 _GradGeom;
                float4 _GradGeom2;
                float4 _GradStop0; float _GradPos0;
                float4 _GradStop1; float _GradPos1;
                float4 _GradStop2; float _GradPos2;
                float4 _GradStop3; float _GradPos3;
                float4 _GradStop4; float _GradPos4;
                float4 _GradStop5; float _GradPos5;
                float4 _GradStop6; float _GradPos6;
                float4 _GradStop7; float _GradPos7;
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
                // clip 链测试空间 = design 坐标（core world_matrix 同空间；root
                // transform 的缩放/y-flip 不介入——inv_frame 直接吃 design 坐标）。
                // OBJECT_MATRIX 时 designWorld 是 objM 后 design 坐标；否则顶点即
                // design 坐标。
                #if defined(OBJECT_MATRIX)
                o.designPos = designWorld.xy + _ObjT.xy;
                #else
                o.designPos = v.pos.xy + _ObjT.xy;
                #endif
                o.color = v.color;
                // SHADOW_BLUR / GRADIENT：core 把几何编码进 uv（SHADOW_BLUR = 顶点 − 形状中心；
                // GRADIENT = box 局部像素坐标，左上原点），须直通 raw uv；TRANSFORM_TEX 会叠
                // _MainTex_ST 缩放/偏移 → 坐标错位（静默依赖 _MainTex_ST==1,1,0,0）。
            #if defined(SHADOW_BLUR) || defined(GRADIENT)
                o.uv = v.uv;
            #else
                o.uv = TRANSFORM_TEX(v.uv, _MainTex);
            #endif
                return o;
            }
            // erfc 近似（Abramowitz-Stegun 7.1.26，精度 ~1.5e-7）。box-shadow blur 用真高斯模糊
            // 指示函数 0.5·erfc(sdf/(σ√2))（长尾、不截断），替 smoothstep 硬截断——匹配浏览器
            // box-shadow 的柔和长尾（更淡、偏移被模糊稀释到不显）。
            float erfc_approx(float x) {
                float z = abs(x);
                float t = 1.0 / (1.0 + 0.3275911 * z);
                float r = t * (0.254829592 + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
                float y = exp(-z * z) * r;
                return (x >= 0.0) ? y : 2.0 - y;
            }
            half4 frag(Vary i) : SV_Target {
                // vertex color 来自 CSS（sRGB 编码）。自适配项目 color space：
                // - Linear：须手动 sRGB→linear（Unity 不自动转 vertex color；与 fgui 一致）。半透明 alpha blend 落在
                //   linear 空间而 CSS 合成在 sRGB → 偏亮发白（业界通病，fgui 同样不解决）。
                // - Gamma：vcol 保持 sRGB 编码值，blend 在 sRGB 空间（匹配 CSS，颜色准）。
                // texture 是 sRGB format 自动转，不重复。alpha 线性不转。
                half4 vcol = i.color;
                #if !defined(UNITY_COLORSPACE_GAMMA)
                half3 sc = vcol.rgb;
                vcol.rgb = (sc <= 0.04045) ? sc / 12.92 : pow((sc + 0.055) / 1.055, 2.4);
                #endif
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
                // outline（stroke）：居中描边，对齐浏览器 -webkit-text-stroke——edge 为带
                // 中心，总可见宽 = 声明宽（±halfW）+ 1px AA（cov = saturate(halfW + 0.5 - |edge|)，
                // 单一坡道，带宽精确）。外半环（edge<0，face 外）over 合成补 alpha；内半环
                // （edge>0，face 内）描边色盖字面（浏览器行为——内半吃进字形）。
                // 旧版用两条 saturate 差分做带——每侧自带 1px 坡道互相叠加，1px 描边实际
                // 渲出 ~2.5px 宽软带（观感"只剩黄色描边、过大"）。
                if (_OutlineWidth > 0.001) {
                    float halfW = _OutlineWidth * 0.5;
                    float cov = saturate(halfW + 0.5 - abs(edge));
                    float oa = cov * _OutlineColor.a;
                    rgb = lerp(rgb, _OutlineColor.rgb, oa);             // 内半环：盖字面色
                    rgb = lerp(rgb, _OutlineColor.rgb, oa * (1.0 - a)); // face 外：外半环
                    a += oa * (1.0 - a) * saturate(0.5 - edge);         // 只在 face 外补 alpha
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
                // 旧 col.a=vcol.a：无 bg-color(vcol.a=0)时全透明丢图。
                // 标准 source-over：a=tex.a+vcol.a·(1−tex.a)；rgb 直通=预乘/a（max 防除零；a=0 像素 Blend 不贡献，rgb 无关）。
                // 有底色不透明(vcol.a=1)：a=1, rgb=图叠底色（与旧公式完全一致，零回归）。
                // 无底色(vcol.a=0)：a=tex.a, rgb=tex.rgb（等价 program:0 图直通，图显透明区透下层）。
                float bgA = tex.a + vcol.a * (1.0 - tex.a);
                float3 bgRgb = ((float3)tex.rgb * tex.a + (float3)vcol.rgb * vcol.a * (1.0 - tex.a)) / max(bgA, 1e-6);
                half4 col = half4(bgRgb, bgA);
                #elif defined(GRADIENT)
                // 背景渐变（program:6/7）：i.uv = box 局部像素坐标（core 编码，raw 直通）。
                // t：linear = (dot(p,dir) − t0)×inv_span（4 角投影归一，CSS 渐变线语义）；
                //     radial = 椭圆归一化距离 sqrt((dx/rx)²+(dy/ry)²)。
                // stops 分段 premultiplied lerp 再反预乘（CSS 渐变插值语义，rgba→transparent 无灰边；
                // 公式与 core sample_gradient 逐字对齐——改一侧必须同步另一侧）。未用槽由 C# 填
                // 末 stop 色 @pos=1 → 段搜索退化正确，无需 count uniform。
                // 末步与 vcol（background-color）source-over 合成：底色垫在渐变下（同 BG_COMPOSITE 公式）。
                float2 gp0 = i.uv;
                float gt;
                if (_GradKind > 0.5) {
                    float2 gd = (gp0 - _GradGeom2.xy) / max(_GradGeom2.zw, float2(1e-4, 1e-4));
                    gt = length(gd);
                } else {
                    gt = saturate((dot(gp0, _GradGeom.xy) - _GradGeom.z) * _GradGeom.w);
                }
                float4 gv = _GradStop0;
                #define GRAD_SEG(A, B) \
                    if (gt >= _GradPos##A && gt < _GradPos##B) { \
                        float gf = saturate((gt - _GradPos##A) / max(_GradPos##B - _GradPos##A, 1e-5)); \
                        float4 gpa = float4(_GradStop##A.rgb * _GradStop##A.a, _GradStop##A.a); \
                        float4 gpb = float4(_GradStop##B.rgb * _GradStop##B.a, _GradStop##B.a); \
                        float4 gpm = lerp(gpa, gpb, gf); \
                        gv = float4(gpm.rgb / max(gpm.a, 1e-4), gpm.a); \
                    }
                GRAD_SEG(0,1) GRAD_SEG(1,2) GRAD_SEG(2,3) GRAD_SEG(3,4)
                GRAD_SEG(4,5) GRAD_SEG(5,6) GRAD_SEG(6,7)
                #undef GRAD_SEG
                if (gt >= _GradPos7) gv = _GradStop7;
                // stops 是 CSS sRGB 值（浏览器在 sRGB 空间插值）；linear 项目与 vcol 同步转 linear。
                #if !defined(UNITY_COLORSPACE_GAMMA)
                half3 gs = gv.rgb;
                gs = (gs <= 0.04045) ? gs / 12.92 : pow((gs + 0.055) / 1.055, 2.4);
                gv.rgb = gs;
                #endif
                float gA = gv.a + vcol.a * (1.0 - gv.a);
                float3 gRgb = ((float3)gv.rgb * gv.a + (float3)vcol.rgb * vcol.a * (1.0 - gv.a)) / max(gA, 1e-6);
                half4 col = half4(gRgb, gA);
                #else
                // image/mesh（program:0）：彩色 texture → tex.rgb × vcol。
                half4 col = tex * vcol;
                #endif
                #if defined(COLOR_FILTER)
                // CSS filter 矩阵定义在 sRGB 空间（contrast -0.25 = sRGB 中点 0.5 偏移）。
                #if !defined(UNITY_COLORSPACE_GAMMA)
                // Linear：col 是 linear → lin→sRGB → 矩阵 → sRGB→linear。max 防 pow 负底数 NaN。
                half3 cfs = col.rgb;
                cfs = (cfs <= 0.0031308) ? cfs * 12.92 : 1.055 * pow(max(cfs, 0.0), 1.0 / 2.4) - 0.055;
                float4x4 cfM = float4x4(_CF0, _CF1, _CF2, _CF3);
                cfs = mul(cfM, float4(cfs, 1.0)).rgb + _CFOff.rgb;
                cfs = (cfs <= 0.04045) ? cfs / 12.92 : pow(max((cfs + 0.055) / 1.055, 0.0), 2.4);
                col.rgb = cfs;
                #else
                // Gamma：col 已是 sRGB 编码值，直接施加矩阵。
                float4x4 cfMg = float4x4(_CF0, _CF1, _CF2, _CF3);
                col.rgb = mul(cfMg, float4(col.rgb, 1.0)).rgb + _CFOff.rgb;
                #endif
                #endif
                // 节点 opacity（从顶点色剥离，per-renderer MPB）。alpha 剥离后 colors.a 不含节点 alpha。
                col.a *= _Alpha;
                #ifdef CLIPPED
                // 多 entry clip 链（#52）：designPos 经 entry 逆矩阵映 clipper box-local
                // （(0,0) = 裁剪器 border box 左上，y-down design 系），按双独立 kind
                // 测试——rectKind（frame1.w：1 直角 AABB / 2 圆角各向异性 SDF）与
                // shapeKind（frame0.w：1 circle SDF / 2 polygon crossing）。同 entry 双
                // kind 并存 = 同元素 overflow:hidden + clip-path（web 交集原义）；链上
                // 全部 entry 都过才保留（web clip 栈交集语义，不坍缩）。
                // rounded/circle 走 SDF 给 1 design px 抗锯齿带；polygon 是硬判定。
                // 与 core hit gate 同一几何语义（resolved::point_in_rounded_rect /
                // ClipShape::contains 的 HLSL 镜像——两侧改须同步）。
                for (int e = 0; e < 4; e++)
                {
                    if (e >= (int)_ClipCount) break;
                    float2 lp = float2(dot(_ClipFrame0[e].xy, i.designPos) + _ClipFrame0[e].z,
                                       dot(_ClipFrame1[e].xy, i.designPos) + _ClipFrame1[e].z);
                    float shapeKind = _ClipFrame0[e].w;
                    float rectKind = _ClipFrame1[e].w;
                    float keep = 1.0;
                    if (rectKind > 1.5)
                    {
                        // 圆角矩形 SDF（像素空间，各角 (rx,ry) 独立）。归一化到半径空间
                        // 做椭圆角 SDF，× min(rx,ry) 回像素量纲（1 design px AA 带）。
                        float2 wh = _ClipRect[e].xy;
                        float2 half2 = wh * 0.5;
                        float2 rel = lp - half2;
                        // y-down：rel.y<0 = 顶行（TL/TR），>=0 底行（BL/BR）。
                        float4 rTop = _ClipRadii0[e];   // (tl_rx, tl_ry, tr_rx, tr_ry)
                        float4 rBot = _ClipRadii1[e];   // (br_rx, br_ry, bl_rx, bl_ry)
                        float2 r = (rel.y < 0.0)
                            ? ((rel.x < 0.0) ? rTop.xy : rTop.zw)
                            : ((rel.x < 0.0) ? rBot.zw : rBot.xy);
                        r = max(r, 1e-4);
                        float2 q = abs(rel) - (half2 - r);
                        float2 qn = q / r;
                        float sdist = length(max(qn, 0.0)) + min(max(qn.x, qn.y), 0.0) - 1.0;
                        float sdf = sdist * min(r.x, r.y);
                        keep = 1.0 - smoothstep(0.0, 1.0, sdf);
                    }
                    else if (rectKind > 0.5)
                    {
                        // 直角 AABB（box-local 0..wh）。
                        float2 wh = _ClipRect[e].xy;
                        keep = step(0.0, lp.x) * step(0.0, lp.y) * step(lp.x, wh.x) * step(lp.y, wh.y);
                    }
                    if (keep > 0.0)
                    {
                        if (shapeKind > 1.5)
                        {
                            // polygon crossing number 奇偶（简单多边形与 web nonzero 一致）。
                            // 点存 _ClipPoly[e×8 + k/2] 两点一 float4，k 奇数取 .zw。
                            int n = (int)_ClipRect[e].z;
                            int pbase = e * 8;
                            int inside = 0;
                            int j = n - 1;
                            for (int k = 0; k < 16; k++)
                            {
                                if (k >= n) break;
                                float4 segA = _ClipPoly[pbase + k / 2];
                                float2 a = ((k & 1) == 0) ? segA.xy : segA.zw;
                                float4 segB = _ClipPoly[pbase + j / 2];
                                float2 b = ((j & 1) == 0) ? segB.xy : segB.zw;
                                if ((a.y > lp.y) != (b.y > lp.y))
                                {
                                    float t = (lp.y - a.y) / (b.y - a.y);
                                    if (lp.x < a.x + t * (b.x - a.x)) inside ^= 1;
                                }
                                j = k;
                            }
                            keep = (float)inside;
                        }
                        else if (shapeKind > 0.5)
                        {
                            float2 d = lp - _ClipCircle[e].xy;
                            keep = 1.0 - smoothstep(0.0, 1.0, length(d) - _ClipCircle[e].z);
                        }
                    }
                    col.a *= keep;
                }
                #endif
                #ifdef SHADOW_BLUR
                // Box-shadow（program=5）：像素空间圆角矩形 SDF + smoothstep 双侧软边。
                // i.uv 由 core 几何编码为「顶点 − 形状中心」（像素量纲），故 _ShadowHalfSize.xy/_ShadowRadius
                // 亦取像素；SDF 公式同 CLIPPED_ROUNDED（归一化半宽 1 换成 _ShadowHalfSize.xy）。
                // 真高斯模糊指示 ind=0.5·erfc(sdf/(σ√2))（σ=blur/2，RmlUi）：1 形状内 → 0 外、边缘 0.5、
                // 长尾不截断。比 smoothstep 硬截断更贴浏览器 box-shadow（柔散、偏移被模糊稀释）。
                // inset 翻 ind 取外侧（内环 + 向心软边）；inset 元素圆角裁剪由 core 几何（rounded_rect mesh）完成。
                float2 p = i.uv;
                float qx = abs(p.x) - _ShadowHalfSize.x + _ShadowRadius;
                float qy = abs(p.y) - _ShadowHalfSize.y + _ShadowRadius;
                // 命名 shadowSdf（非 sdf）：CLIPPED_ROUNDED 块也声明 sdf，I1 路径（shadow 在圆角
                // overflow 容器）两 keyword 共启 → 同名 redefinition 编译错。两块各用专名避撞。
                float shadowSdf = length(max(float2(qx, qy), 0.0)) + min(max(qx, qy), 0.0) - _ShadowRadius;
                float sig = max(_ShadowSigma, 0.0001);
                // ind=0.5·erfc(sdf/(σ√2))：1 形状内 → 0 外、边缘 0.5、高斯长尾不截断。
                float ind = 0.5 * erfc_approx(shadowSdf / (sig * 1.41421356));
                col.a *= (_ShadowInset > 0.5) ? (1.0 - ind) : ind;
                #endif
                return col;
            }
            ENDHLSL
        }
    }
}
