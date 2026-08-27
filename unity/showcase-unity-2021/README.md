# showcase-unity-2021 — Unity 2021.3 回归工程

消费者路径验收样板：干净 URP 工程 + 本地安装 `com.ikat.unity`（file: 依赖），
验证包在 **Unity 2021.3.45f2**（最低支持版本）上的编译、渲染、输入、导航。

## 配置要点

- 编辑器：2021.3.45f2（Personal 可用的最后一个标准 LTS 构建）
- URP 12.1.15、Linear→**Gamma** 色彩空间（CSS 合成语义更准，见 shader 注释）
- Active Input Handling = Both（输入双路径三模式全兼容的验证环境）
- `Assets/Scenes/SampleScene`：`IkatUI` GO（IkatStageDriver 1080×1920→**1920×1080** + IkatInputCollector + ShowcaseRunner）

## Bundles 不入库（33MB，与 showcase-unity 同份产物）

首次使用先拷贝：

```
cp -r ../showcase-unity/Assets/Bundles Assets/Bundles
cp ../showcase-unity/Assets/Bundles.meta Assets/Bundles.meta
```

或用打包器重新产出：`cargo run -p ikat_pkg -- build <workspace>`。

## 验证清单

- [x] 包编译零错误（含 showcase 全脚本）
- [x] EditMode 测试：输入/事件相关全过（既有 10 个 FrameBlob v13 漂移失败为全版本已知项）
- [x] PlayMode：home 全屏渲染、nav-card 点击切页、悬浮态
- [ ] IME：form 页文本框中文组字/候选窗/上屏（手测）
