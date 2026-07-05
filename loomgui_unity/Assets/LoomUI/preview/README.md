# Showcase 浏览器预览

双击 `../showcase/home.html` 即可在浏览器预览 showcase（视觉对照用，非行为镜像）。

## 可信（≈ Unity 渲染）
flex 布局/方向/gap/justify/align、px/% 尺寸、color/opacity/border/radius、
background-image/size、filter、transform、overflow:scroll、九宫 border-image-slice、列表几何/滚动。

## 近似（抓布局偏差，不卡像素）
- tween 动画：CSS transition 近似，不逐曲线对齐 ease。
- 文本换行/字距像素：Chrome 文本引擎 vs LoomGUI(unicode-linebreak)，换行点会偏。
- drag/longpress/key 触发条件：浏览器事件近似。
- NativeHost（外部 Cube GO）：`#model-slot` 显示占位文本，无法复刻。
- overlay mail/tips：用 loom-preview.js 内嵌模板。

## 维护
- 改了 showcase HTML：刷新浏览器即可。
- 改了 `../showcase/mail.html` 或 `tips_toast.html`：须同步 `loom-preview.js` 里的 `TEMPLATES`。
