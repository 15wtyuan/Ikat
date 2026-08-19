using System.Runtime.InteropServices;
using UnityEngine;
using LoomGUI.Bindings;
#if ENABLE_INPUT_SYSTEM
using UnityEngine.InputSystem;
using UnityEngine.InputSystem.LowLevel;
#endif

namespace LoomGUI
{
    /// 输入采集：Unity 指针（鼠标+触摸）→ PointerEvent[] → loomgui_stage_set_input。
    /// screen→design 映射 + y-flip（Unity 左下原点 → LoomGUI 左上原点 design）。
    /// 兼容新旧输入系统：ENABLE_INPUT_SYSTEM 宏（Player Settings Active Input Handling=New/Both）走
    /// InputSystem API（Mouse/Touchscreen 轮询 + Keyboard 的 onTextInput/onIMECompositionChange 事件），
    /// 否则走旧 UnityEngine.Input（inputString/compositionString 等）。三种 Active Input Handling
    /// 配置（Old / New / Both）全支持——键盘族若只走单路径，New-only 工程每帧抛
    /// InvalidOperationException，开箱即用就断了。
    [ExecuteAlways]
    public unsafe class LoomInputCollector : MonoBehaviour
    {
        /// <summary>
        /// 设计分辨率（design px）+ safe-area 开关：UnityLoomBackend.CollectInput / CollectWheel 读此做
        /// screen→design 映射（替代旧 LoomStage.DesignSize/UseSafeArea 路径——P2 后端分层不再持 LoomStage）。
        /// 由当前 Driver（LoomStageDriver 或 P2.5 LoomHost Driver）在 Awake 注入。
        /// </summary>
        internal UnityEngine.Vector2 DesignSize { get; set; }
        internal bool UseSafeArea { get; set; }

        /// <summary>
        /// 滚轮手感倍率（默认 1 = 浏览器同档基准）：滚轮 delta 的最终乘数，PlayMode 中
        /// Inspector 实时调。基准依据：Windows 一格 3 行文本 ≈ 60-75px、Chrome ≈ 100-120px；
        /// 1920×1080 设计视口下 100 design px/格 ≈ 10 格滚完一屏。设备事件密度不同导致
        /// 的手感差异在此调（逐设备手感值，不作为跨设备默认）。
        /// </summary>
        [Tooltip("滚轮手感倍率：1 = 一格约 100 design px（浏览器同档）。个人手感在此调。")]
        [SerializeField] float _wheelScrollSpeed = 1f;
        /// <summary>滚轮手感倍率（<see cref="_wheelScrollSpeed"/> 的只读出口）。</summary>
        public float WheelScrollSpeed => _wheelScrollSpeed;

        /// 上帧聚焦节点缓存：IME mode 仅在焦点真正转换时切换，避免每帧重设（移动端/
        /// WebGL IME 状态切换昂贵）。0xFFFFFFFF = 无聚焦。
        private uint _lastFocused = 0xFFFF_FFFFu;

#if ENABLE_INPUT_SYSTEM
        /// 本帧字符输入缓冲：Keyboard.onTextInput 逐字符事件触发（两帧之间可多次），
        /// CollectText 每帧消费后清空。替代旧路径 Input.inputString 的帧轮询。
        readonly System.Text.StringBuilder _textBuf = new System.Text.StringBuilder();

        /// 最新 IME 预编辑串缓存：onIMECompositionChange 每次变更发当前完整串、
        /// 上屏/重置发空串（与旧 Input.compositionString 同构），CollectComposition 每帧读。
        string _composition = "";

        /// 已订阅事件的键盘设备。Keyboard.current 是设备实例——热插拔会换新实例，
        /// 事件挂实例上，须跟随 onDeviceChange 重订，否则换键盘后 text/IME 静默失联。
        Keyboard _subKb;

        void OnEnable()
        {
            InputSystem.onDeviceChange += OnDeviceChange;
            Subscribe(Keyboard.current);
        }

        void OnDisable()
        {
            InputSystem.onDeviceChange -= OnDeviceChange;
            Unsubscribe();
        }

        /// 键盘设备变更（热插拔/重连/切布局等）：Keyboard.current 可能换新实例，事件挂实例上，
        /// 统一重订到当前键盘（current 为 null 时 Subscribe 内部 no-op）。不能只认
        /// Added/Reconnected——ConfigurationChanged（Windows 切键盘布局）等变更只退订不重订，
        /// text/IME 从此静默失联。
        void OnDeviceChange(InputDevice device, InputDeviceChange change)
        {
            if (device is Keyboard)
            {
                Unsubscribe();
                Subscribe(Keyboard.current);
            }
        }

        void Subscribe(Keyboard kb)
        {
            if (kb == null) return;
            kb.onTextInput += OnTextInput;
            kb.onIMECompositionChange += OnIMEComposition;
            _subKb = kb;
        }

        void Unsubscribe()
        {
            if (_subKb == null) return;
            _subKb.onTextInput -= OnTextInput;
            _subKb.onIMECompositionChange -= OnIMEComposition;
            _subKb = null;
        }

        void OnTextInput(char ch) => _textBuf.Append(ch);

        void OnIMEComposition(IMECompositionString composition) => _composition = composition.ToString();
#endif

        /// screen→design 映射，与 LoomStageDriver.ConfigureTransforms 逐项逆（同一 sf 居中公式）。
        /// 前向（design→screen，见 Driver 的根变换注释）：
        ///   screen.x = offX    + dx*sf     其中 offX = area.x + (area.width  - dw*sf)*0.5
        ///   screen.y = offYTop - dy*sf     其中 offYTop = area.y + area.height
        /// 逆：
        ///   dx = (screen.x - offX)    / sf
        ///   dy = (offYTop - screen.y) / sf
        /// sf = min(area.w/dw, area.h/dh)【统一缩放，与渲染一致】。
        /// useSafeArea=false 时 area 退回全屏。
        public static UnityEngine.Vector2 ScreenToDesign(UnityEngine.Vector2 screen, Vector2Int screenSize, UnityEngine.Vector2 rootSize, UnityEngine.Rect area, bool useSafeArea)
        {
            float sw = screenSize.x > 0 ? screenSize.x : 1;
            float sh = screenSize.y > 0 ? screenSize.y : 1;
            UnityEngine.Rect a = useSafeArea ? area : new UnityEngine.Rect(0, 0, sw, sh);
            // 防御：safeArea 可能零宽高（编辑器未配屏）→ 退回全屏
            if (a.width <= 0f || a.height <= 0f) a = new UnityEngine.Rect(0, 0, sw, sh);
            float dw = rootSize.x > 0 ? rootSize.x : 1;
            float dh = rootSize.y > 0 ? rootSize.y : 1;
            // 统一 shrink-to-fit 缩放（与 ComputeRootTransform 同一式）。
            float sf = Mathf.Min(a.width / dw, a.height / dh);
            sf = sf > 0 ? sf : 1f;   // 除零保护
            float offX = a.x + (a.width - dw * sf) * 0.5f;
            float offYTop = a.y + a.height;
            float dx = (screen.x - offX) / sf;
            float dy = (offYTop - screen.y) / sf;
            return new UnityEngine.Vector2(dx, dy);
        }

        /// design→screen 映射，用于 IME 候选窗定位（compositionCursorPos）。
        /// 注意：与 ScreenToDesign 逆映射公式不同——compositionCursorPos 的坐标系因平台而异
        /// （Unity Editor 实测为左上原点 y-down，与 Input.mousePosition 左下原点相反），
        /// 与 design 同为左上 y-down，故直接线性映射不 y-flip，Y 偏移用 root 居中式
        /// (offY = area.y + (area.h - dh*sf)*0.5) 而非 ScreenToDesign 的 offYTop。
        /// ⚠ 若目标 Player 平台 compositionCursorPos 改用左下原点 y-up（与 mousePosition 同），
        /// 则需改为 offYTop - design.y*sf（加 y-flip）——发布前须在目标 Player 实测确认。
        public static UnityEngine.Vector2 DesignToScreen(UnityEngine.Vector2 design, Vector2Int screenSize, UnityEngine.Vector2 rootSize, UnityEngine.Rect area, bool useSafeArea)
        {
            float sw = screenSize.x > 0 ? screenSize.x : 1;
            float sh = screenSize.y > 0 ? screenSize.y : 1;
            UnityEngine.Rect a = useSafeArea ? area : new UnityEngine.Rect(0, 0, sw, sh);
            if (a.width <= 0f || a.height <= 0f) a = new UnityEngine.Rect(0, 0, sw, sh);
            float dw = rootSize.x > 0 ? rootSize.x : 1;
            float dh = rootSize.y > 0 ? rootSize.y : 1;
            float sf = Mathf.Min(a.width / dw, a.height / dh);
            sf = sf > 0 ? sf : 1f;
            float offX = a.x + (a.width - dw * sf) * 0.5f;
            // compositionCursorPos 在 Editor 中是左上原点 y-down（OS 屏幕语义，与 Input.mousePosition
            // 左下原点相反——IME 候选窗定位实测：默认 (0,0) 显示在编辑器左上）。design 也是左上
            // y-down，直接映射不 y-flip。design root 居中在 area：顶部偏移 (area.h - root*sf)/2。
            float offY = a.y + (a.height - dh * sf) * 0.5f;
            float screenX = offX + design.x * sf;
            float screenY = offY + design.y * sf;
            return new UnityEngine.Vector2(screenX, screenY);
        }

        /// 采集本帧指针（鼠标+触摸）→ set_input。鼠标 touch_id=-1（slot0），触摸 touch_id=fingerId（slot1-4）。
        /// 鼠标+触摸可同帧共存（带触摸屏桌面）；EditMode 无 Touchscreen 跳过触摸。
        public void Collect(System.IntPtr stage, UnityEngine.Vector2 rootSize, bool useSafeArea)
        {
            if (stage == System.IntPtr.Zero) return;
            var events = new System.Collections.Generic.List<Bindings.PointerEvent>();
            var screenSize = new Vector2Int(Screen.width, Screen.height);
            UnityEngine.Rect safeArea = Screen.safeArea;

#if ENABLE_INPUT_SYSTEM
            // 鼠标（主指，touch_id=-1）
            if (Mouse.current != null)
            {
                var screen = Mouse.current.position.ReadValue();
                byte kind = 2;
                if (Mouse.current.leftButton.wasPressedThisFrame) kind = 0;
                else if (Mouse.current.leftButton.wasReleasedThisFrame) kind = 1;
                var d = ScreenToDesign(screen, screenSize, rootSize, safeArea, useSafeArea);
                events.Add(new Bindings.PointerEvent { kind = kind, button = 0, pad0 = 0, pad1 = 0, touch_id = -1, x = d.x, y = d.y });
            }
            // 触摸（多指）。TouchPhase 在 UnityEngine.InputSystem（非 LowLevel——坑：1.19 包 TouchPhase 不在 LowLevel）。
            if (Touchscreen.current != null)
            {
                foreach (var touch in Touchscreen.current.touches)
                {
                    if (touch == null) continue;
                    var phase = touch.phase.ReadValue();
                    if (phase == UnityEngine.InputSystem.TouchPhase.Stationary) continue;
                    byte kind = 2;
                    if (phase == UnityEngine.InputSystem.TouchPhase.Began) kind = 0;
                    else if (phase == UnityEngine.InputSystem.TouchPhase.Ended) kind = 1;
                    else if (phase == UnityEngine.InputSystem.TouchPhase.Canceled) kind = 3;   // Canceled
                    var screen = touch.position.ReadValue();
                    var d = ScreenToDesign(screen, screenSize, rootSize, safeArea, useSafeArea);
                    events.Add(new Bindings.PointerEvent { kind = kind, button = 0, pad0 = 0, pad1 = 0, touch_id = touch.touchId.ReadValue(), x = d.x, y = d.y });
                }
            }
#else
            // 旧输入系统
            var mscreen = Input.mousePosition;
            byte mkind = 2;
            if (Input.GetMouseButtonDown(0)) mkind = 0;
            else if (Input.GetMouseButtonUp(0)) mkind = 1;
            var md = ScreenToDesign(mscreen, screenSize, rootSize, safeArea, useSafeArea);
            events.Add(new Bindings.PointerEvent { kind = mkind, button = 0, pad0 = 0, pad1 = 0, touch_id = -1, x = md.x, y = md.y });
            foreach (var t in Input.touches)
            {
                if (t.phase == UnityEngine.TouchPhase.Stationary) continue;
                byte kind = 2;
                if (t.phase == UnityEngine.TouchPhase.Began) kind = 0;
                else if (t.phase == UnityEngine.TouchPhase.Ended) kind = 1;
                else if (t.phase == UnityEngine.TouchPhase.Canceled) kind = 3;   // Canceled
                var d = ScreenToDesign(t.position, screenSize, rootSize, safeArea, useSafeArea);
                events.Add(new Bindings.PointerEvent { kind = kind, button = 0, pad0 = 0, pad1 = 0, touch_id = t.fingerId, x = d.x, y = d.y });
            }
#endif
            if (events.Count == 0)
            {
                Native.loomgui_stage_set_input((Bindings.StageHandle*)stage, null, 0);
                return;
            }
            // csbindgen 生成的 set_input 取 PointerEvent*（raw 指针，非托管数组）+ nuint len。
            // events.ToArray() 是托管 PointerEvent[] —— 必须 fixed 钉住首元素取指针传入。
            var arr = events.ToArray();
            fixed (Bindings.PointerEvent* p = arr)
            {
                Native.loomgui_stage_set_input((Bindings.StageHandle*)stage, p, (nuint)arr.Length);
            }
        }

        /// 采集本帧键盘 → set_key_input。KeyDown/Up 事件 + modifiers。
        /// 新旧输入系统双路径：新路径轮询 Keyboard[Key]（wasPressed/wasReleasedThisFrame，
        /// 与鼠标按钮同帧时机），旧路径轮询 Input.GetKeyDown/Up。两路径发码统一取
        /// KeyCode 数值（core key_code=(uint)KeyCode 契约，FFI 语义不随工程输入配置变——
        /// InputSystem.Key 与 KeyCode 数值不同，不能直发）。本帧无键事件 →
        /// set_key_input(null,0)（core 无键盘输入）。
        public void CollectKeys(System.IntPtr stage)
        {
            if (stage == System.IntPtr.Zero) return;
            var keys = new System.Collections.Generic.List<Bindings.KeyEvent>();
            byte mods = CurrentModifiers();
#if ENABLE_INPUT_SYSTEM
            var kb = Keyboard.current;
            if (kb != null)
            {
                for (int i = 0; i < KeyList.Length; i++)
                {
                    var ctrl = kb[NewKeyList[i]];
                    bool down = ctrl.wasPressedThisFrame;
                    bool up = ctrl.wasReleasedThisFrame;
                    if (down || up)
                        keys.Add(new Bindings.KeyEvent { key_code = (uint)KeyList[i], modifiers = mods, is_down = down, pad0 = 0, pad1 = 0 });
                }
            }
#else
            foreach (UnityEngine.KeyCode kc in KeyList)
            {
                bool down = UnityEngine.Input.GetKeyDown(kc);
                bool up = UnityEngine.Input.GetKeyUp(kc);
                if (down || up)
                    keys.Add(new Bindings.KeyEvent { key_code = (uint)kc, modifiers = mods, is_down = down, pad0 = 0, pad1 = 0 });
            }
#endif
            if (keys.Count == 0)
            {
                Native.loomgui_stage_set_key_input((Bindings.StageHandle*)stage, null, 0);
                return;
            }
            var arr = keys.ToArray();
            fixed (Bindings.KeyEvent* p = arr)
            {
                Native.loomgui_stage_set_key_input((Bindings.StageHandle*)stage, p, (nuint)arr.Length);
            }
        }

        /// 采集本帧字符输入 → set_text_input（UTF-32 codepoints）。已映射可打印字符
        /// （数字/字母/符号；IME 上屏结果字符同路）。空串 → set_text_input(null,0)
        /// （core 无字符输入）。与 CollectKeys 互补：keydown 通道走物理键（控制键
        /// Backspace/Delete/方向/翻页），textinput 通道走映射好的字符。
        /// 双路径：新路径消费 OnTextInput 事件缓存（每帧清空），旧路径读 Input.inputString。
        public void CollectText(System.IntPtr stage)
        {
            if (stage == System.IntPtr.Zero) return;
#if ENABLE_INPUT_SYSTEM
            string s = _textBuf.Length == 0 ? "" : _textBuf.ToString();
            _textBuf.Clear();
#else
            string s = UnityEngine.Input.inputString;
#endif
            if (string.IsNullOrEmpty(s))
            {
                Native.loomgui_stage_set_text_input((Bindings.StageHandle*)stage, null, 0);
                return;
            }
            // string → UTF-32 codepoints（代理对占 2 char → 1 codepoint）。
            var cps = new System.Collections.Generic.List<uint>();
            for (int i = 0; i < s.Length; )
            {
                int code = char.ConvertToUtf32(s, i);
                cps.Add((uint)code);
                i += char.IsSurrogatePair(s, i) ? 2 : 1;
            }
            var arr = cps.ToArray();
            fixed (uint* p = arr)
            {
                Native.loomgui_stage_set_text_input((Bindings.StageHandle*)stage, p, (nuint)arr.Length);
            }
        }

        /// 采集本帧 IME composition（系统输入法预编辑串）→ set_composition（UTF-8）。
        /// 组字串语义（两输入系统同构）：组字中非空（完整预编辑串），组字完成变空，
        /// 结果字符经 CollectText 上屏。聚焦文本框时显式开 IME（IME Auto 模式基于
        /// UnityEngine.GUI TextField，LoomGUI 自绘不触发，故须显式 On），失焦关 IME。
        /// 核心用 e.cursor 定位组字插入点（FFI pos 参数忽略，C# 传 0）。
        /// 双路径：新路径 Keyboard.SetIMEEnabled/SetIMECursorPosition（左上原点、像素、
        /// y 向下——与旧 compositionCursorPos 编辑器语义同坐标系，DesignToScreen 直供）
        /// + onIMECompositionChange 缓存；旧路径 Input.imeCompositionMode/compositionString。
        public void CollectComposition(System.IntPtr stage)
        {
            if (stage == System.IntPtr.Zero) return;
            Bindings.StageHandle* h = (Bindings.StageHandle*)stage;
#if ENABLE_INPUT_SYSTEM
            var kb = Keyboard.current;
#endif
            uint focused = Native.loomgui_stage_focused_node(h);
            const uint NONE = 0xFFFF_FFFFu;
            if (focused == NONE)
            {
                // 无聚焦文本框：仅在从聚焦转出时关 IME（避免每帧重设，移动端/WebGL IME
                // 状态切换昂贵）。
                if (_lastFocused != NONE)
                {
#if ENABLE_INPUT_SYSTEM
                    kb?.SetIMEEnabled(false);
                    // 新路径组字串是自维护缓存（旧路径 compositionString 是 Unity 全局状态，
                    // IME Off 即被重置）——不清则组字中途失焦、再聚焦下一个文本框时旧串复活。
                    _composition = "";
#else
                    UnityEngine.Input.imeCompositionMode = UnityEngine.IMECompositionMode.Off;
#endif
                    _lastFocused = NONE;
                }
                return;
            }
            // 聚焦文本框：仅在从无聚焦（或换节点）转入时开 IME。
            if (_lastFocused != focused)
            {
#if ENABLE_INPUT_SYSTEM
                kb?.SetIMEEnabled(true);
#else
                UnityEngine.Input.imeCompositionMode = UnityEngine.IMECompositionMode.On;
#endif
                _lastFocused = focused;
            }
            // IME 候选窗定位：读光标世界矩形（design 空间，左上原点）→ screen（Unity 左下原点）。
            // 候选窗跟随光标，定位在光标底部（r.y + r.h，候选窗在下方显示）。
            Bindings.CursorRectRepr r;
            if (Native.loomgui_stage_get_cursor_rect(h, focused, &r) == 0)
            {
                var ss = new Vector2Int(Screen.width, Screen.height);
                UnityEngine.Rect sa = Screen.safeArea;
                var screenPos = DesignToScreen(
                    new UnityEngine.Vector2(r.x, r.y + r.h),
                    ss, this.DesignSize, sa, this.UseSafeArea);
#if ENABLE_INPUT_SYSTEM
                kb?.SetIMECursorPosition(screenPos);
#else
                UnityEngine.Input.compositionCursorPos = screenPos;
#endif
            }
            // 组字串 → set_composition。组字中设预编辑串（核心 display 拼组字显示），
            // 组字完成（空串）清预编辑（CollectText 同帧 insert 结果字符）。
#if ENABLE_INPUT_SYSTEM
            string comp = _composition;
#else
            string comp = UnityEngine.Input.compositionString;
#endif
            byte[] bytes = System.Text.Encoding.UTF8.GetBytes(comp ?? "");
            if (bytes.Length == 0)
            {
                Native.loomgui_stage_set_composition(h, focused, null, 0, 0);
            }
            else
            {
                fixed (byte* p = bytes)
                {
                    Native.loomgui_stage_set_composition(h, focused, p, (nuint)bytes.Length, 0);
                }
            }
        }

        /// 采集本帧滚轮 → set_wheel_input。tick 前调；累积式（多次调合并）。
        /// 新旧输入系统双路径：滚轮用旧 Input.mouseScrollDelta 或新 Mouse.current.scroll。
        /// 归一 delta → ±1/格：旧 Input.mouseScrollDelta 已 ≈ ±1/格；新系统 120 像素/格除 120。
        /// 鼠标不在 UI 上也可滚——hit test 由 Rust 侧做（只在悬停的 scroll 容器响应）。
        //
        // 签名说明：旧签名 CollectWheel(LoomStage stage) 在 P2.2 拆为 (stagePtr, ctx)——
        // UnityLoomBackend 不持 LoomStage（只持 IntPtr stage handle + LoomInputCollector ctx），
        // DesignSize/UseSafeArea 由 ctx 读（替代 stage.DesignSize/UseSafeArea）。
        public static void CollectWheel(System.IntPtr stagePtr, LoomInputCollector ctx)
        {
            if (ctx == null || stagePtr == System.IntPtr.Zero) return;

            float dy = 0f;
#if ENABLE_INPUT_SYSTEM
            // notch 模式判定阈值（raw）：120（经典 notch 一格的 raw 值）取半——单事件
            // ≥ 半格即判 notch 类设备。经典 notch 每事件 ±120 或其倍数，不会落在阈值下；
            // 高分辨率滚轮/触控板常规事件 raw≈1-10、惯性峰值可上百——峰值落入 notch
            // 模式是有意的：透传当量下峰值 raw 会一步跨几十格，压回 Round(raw/120) 反而
            // 接近 notch 鼠标的惯性体感。
            const float notchThresholdRaw = 60f;
            var v = UnityEngine.InputSystem.Mouse.current?.scroll?.ReadValue() ?? UnityEngine.Vector2.zero;
            // 双模映射（免定标）：notch 类（120 raw = 1 格）取整；小事件流（精滚轮驱动，
            // 实测每格 raw≈1.5-2）直接透传——1 raw = 1 格当量，逐事件小步进（core 侧
            // clamp+tween 链动，等效连续滚动）。两档语义统一为
            // 「dy 单位 = 格，core 每 100 design px/格」。
            if (Mathf.Abs(v.y) >= notchThresholdRaw)
                dy = Mathf.Round(v.y / 120f);
            else if (!Mathf.Approximately(v.y, 0f))
                dy = v.y;
            dy *= ctx.WheelScrollSpeed;
#else
            dy = Input.mouseScrollDelta.y;  // 旧系统已 ≈ ±1/格
#endif
            if (Mathf.Approximately(dy, 0f)) return;

            UnityEngine.Vector2 screenPos;
#if ENABLE_INPUT_SYSTEM
            screenPos = UnityEngine.InputSystem.Mouse.current?.position?.ReadValue() ?? UnityEngine.Vector2.zero;
#else
            screenPos = Input.mousePosition;
#endif

            var ss = new Vector2Int(Screen.width, Screen.height);
            UnityEngine.Rect sa = Screen.safeArea;
            var pos = ScreenToDesign(screenPos, ss, ctx.DesignSize, sa, ctx.UseSafeArea);

            var ev = new Bindings.WheelEvent { x = pos.x, y = pos.y, delta_x = 0f, delta_y = dy };
            // 栈局部值类型直接 & 取址（CS0213：栈上已固定，无需 fixed）。
            Native.loomgui_stage_set_wheel_input((Bindings.StageHandle*)stagePtr, &ev, 1);
        }

        /// 当前 modifiers 位掩码（bit0=shift/bit1=ctrl/bit2=alt）。core MOD_SHIFT/CTRL/ALT 同值。
        static byte CurrentModifiers()
        {
            byte m = 0;
#if ENABLE_INPUT_SYSTEM
            var kb = UnityEngine.InputSystem.Keyboard.current;
            if (kb == null) return 0;
            if (kb.leftShiftKey.isPressed || kb.rightShiftKey.isPressed) m |= 0x01;
            if (kb.leftCtrlKey.isPressed || kb.rightCtrlKey.isPressed) m |= 0x02;
            if (kb.leftAltKey.isPressed || kb.rightAltKey.isPressed) m |= 0x04;
#else
            if (UnityEngine.Input.GetKey(UnityEngine.KeyCode.LeftShift) || UnityEngine.Input.GetKey(UnityEngine.KeyCode.RightShift)) m |= 0x01;
            if (UnityEngine.Input.GetKey(UnityEngine.KeyCode.LeftControl) || UnityEngine.Input.GetKey(UnityEngine.KeyCode.RightControl)) m |= 0x02;
            if (UnityEngine.Input.GetKey(UnityEngine.KeyCode.LeftAlt) || UnityEngine.Input.GetKey(UnityEngine.KeyCode.RightAlt)) m |= 0x04;
#endif
            return m;
        }

        /// 采集的键白名单（Tab + 字母 + Enter/Space/Esc/方向 + 数字）。避免全 KeyCode 枚举遍历（数百个）开销。
        /// 显式白名单而非全枚举——绝大多数键业务不关心，白名单够用且省 CPU。
        internal static readonly UnityEngine.KeyCode[] KeyList = {
            UnityEngine.KeyCode.Tab,
            // 编辑控制键：Backspace/Delete（删字符）+ Home/End（行首/尾）。core KEY_DELETE=323
            // 须与此处 KeyCode.Delete(323) 一致；缺失则文本控件无法删字符。
            UnityEngine.KeyCode.Backspace, UnityEngine.KeyCode.Delete,
            UnityEngine.KeyCode.Return, UnityEngine.KeyCode.Space, UnityEngine.KeyCode.Escape,
            UnityEngine.KeyCode.LeftArrow, UnityEngine.KeyCode.RightArrow, UnityEngine.KeyCode.UpArrow, UnityEngine.KeyCode.DownArrow,
            UnityEngine.KeyCode.Home, UnityEngine.KeyCode.End,
            UnityEngine.KeyCode.A, UnityEngine.KeyCode.B, UnityEngine.KeyCode.C, UnityEngine.KeyCode.D, UnityEngine.KeyCode.E,
            UnityEngine.KeyCode.F, UnityEngine.KeyCode.G, UnityEngine.KeyCode.H, UnityEngine.KeyCode.I, UnityEngine.KeyCode.J,
            UnityEngine.KeyCode.K, UnityEngine.KeyCode.L, UnityEngine.KeyCode.M, UnityEngine.KeyCode.N, UnityEngine.KeyCode.O,
            UnityEngine.KeyCode.P, UnityEngine.KeyCode.Q, UnityEngine.KeyCode.R, UnityEngine.KeyCode.S, UnityEngine.KeyCode.T,
            UnityEngine.KeyCode.U, UnityEngine.KeyCode.V, UnityEngine.KeyCode.W, UnityEngine.KeyCode.X, UnityEngine.KeyCode.Y, UnityEngine.KeyCode.Z,
            UnityEngine.KeyCode.Alpha0, UnityEngine.KeyCode.Alpha1, UnityEngine.KeyCode.Alpha2, UnityEngine.KeyCode.Alpha3, UnityEngine.KeyCode.Alpha4,
            UnityEngine.KeyCode.Alpha5, UnityEngine.KeyCode.Alpha6, UnityEngine.KeyCode.Alpha7, UnityEngine.KeyCode.Alpha8, UnityEngine.KeyCode.Alpha9,
        };

#if ENABLE_INPUT_SYSTEM
        /// KeyList 的 InputSystem.Key 平行数组（同下标一一对应）。命名差异对照：
        /// Alpha0-9↔Digit0-9、Return↔Enter；其余白名单键两枚举同名。CollectKeys 新路径
        /// 按此轮询，发码仍取 KeyCode 数值（core key_code 契约，见 CollectKeys 注释）。
        internal static readonly Key[] NewKeyList = {
            Key.Tab,
            // 编辑控制键：Backspace/Delete（删字符）+ Home/End（行首/尾）。发码取 KeyList
            // 同下标 KeyCode 数值（Delete=323 与 core KEY_DELETE 对齐）。
            Key.Backspace, Key.Delete,
            Key.Enter, Key.Space, Key.Escape,
            Key.LeftArrow, Key.RightArrow, Key.UpArrow, Key.DownArrow,
            Key.Home, Key.End,
            Key.A, Key.B, Key.C, Key.D, Key.E,
            Key.F, Key.G, Key.H, Key.I, Key.J,
            Key.K, Key.L, Key.M, Key.N, Key.O,
            Key.P, Key.Q, Key.R, Key.S, Key.T,
            Key.U, Key.V, Key.W, Key.X, Key.Y, Key.Z,
            Key.Digit0, Key.Digit1, Key.Digit2, Key.Digit3, Key.Digit4,
            Key.Digit5, Key.Digit6, Key.Digit7, Key.Digit8, Key.Digit9,
        };
#endif
    }
}
