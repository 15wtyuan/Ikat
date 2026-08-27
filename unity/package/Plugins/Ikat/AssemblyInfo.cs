// 暴露 Ikat.Bindings 的 internal（csbindgen 生成的 Native / StageHandle）给 Ikat.Runtime + Ikat.Tests。
// csbindgen 默认生成 internal 类型；IkatHost / UnityIkatBackend（Ikat.Runtime）+ 路由测（Ikat.Tests，BuildStage）
// 需跨程序集调用 Native.ikat_stage_new/load_html/free。
using System.Runtime.CompilerServices;

[assembly: InternalsVisibleTo("Ikat.Runtime")]
[assembly: InternalsVisibleTo("Ikat.Tests")]
