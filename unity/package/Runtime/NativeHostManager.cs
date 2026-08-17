using System.Collections.Generic;
using LoomGUI.Bindings;
using UnityEngine;

namespace LoomGUI
{
    /// <summary>
    /// NativeHost：外部 GO 跟随 UI 节点 transform + 显隐 + 排序。两层结构：
    ///   - per-node wrapper GO：Sync 每帧设其 transform 跟随 UI 节点 world。
    ///   - 用户 GO 挂 wrapper 下，自身 transform（含 scale 放大）完全用户控制，不被 Sync 覆盖。
    ///
    /// 渲染顺序：
    ///   - GO + wrapper layer = LoomUILayer（UI 相机渲染）
    ///   - GO（Mesh/SkinnedMesh/ParticleSystem Renderer）material renderQueue=3000（Transparent，跟 UI 同队列）
    ///   - GO sortingOrder = 节点 sort_key + HostSortOrderLift（盖过宿主上方的合并 UI 块，见常量注释）
///
    /// LoomGUI root localScale=(sf,-sf,sf) 在 transform 做 y-flip。
    /// GO 直挂 root → handedness flip → 3D mesh winding 反 → 被 Cull Back 剔除。
    /// 解法：_container 挂 root、localScale=(1,-1,1)，worldScale=(sf,sf,sf) positive → 子树 handedness 正常。
    /// </summary>
    internal sealed class NativeHostManager
    {
        /// GO sortingOrder 在节点 sort_key 之上的抬升量。UI mesh 经 merge_meshes 合并后
        /// blob 的 sort_key 是合并期重编号、不再是 node DFS 序——宿主节点区域上层的合并
        /// 背景块 key 可远超宿主 key，严格按 sort_key 排序会把 GO 整个压在底下（Unity 跨
        /// sortingOrder 不做 z 距离 tiebreak），抬 1000 跨过宿主上方的本地合并块。
        /// 失效边界：宿主节点绘制序之后再有超过 Lift 个合并渲染块、或宿主 slot 上方
        /// Lift 序内存在需盖住 GO 的重叠 UI（弹层）时排序不再成立——届时须改为 GO 参与
        /// 合并编号的精确穿插方案，勿再加大 Lift。
        internal const int HostSortOrderLift = 1000;

        private readonly Dictionary<uint, GameObject> _bindings = new();   // node_id → 用户 GO
        private readonly Dictionary<uint, GameObject> _wrappers = new();   // node_id → wrapper GO（跟随 UI）
        private Transform _root;
        private GameObject _container;  // 挂 root、localScale (1,-1,1) 翻正 handedness

        public void Init(Transform root)
        {
            _root = root;
            // _container：挂 root（继承 design→world position），localScale (1,-1,1) 抵消 root y-flip。
            // container.worldScale = root.scale × (1,-1,1) = (sf,sf,sf) positive → 子树 handedness 正常。
            _container = new GameObject("LoomNativeHost") { hideFlags = HideFlags.DontSaveInEditor };
            _container.transform.SetParent(root, false);
            _container.transform.localScale = new Vector3(1, -1, 1);
            _container.transform.localRotation = Quaternion.identity;
            _container.transform.localPosition = Vector3.zero;
            _container.layer = root.gameObject.layer;  // LoomUILayer
        }

        public void Bind(uint nodeId, GameObject go)
        {
            if (go == null) return;
            Unbind(nodeId);
            // per-node wrapper：Sync 设其 transform 跟随 UI 节点。
            var wrapper = new GameObject("LoomNH_" + nodeId) { hideFlags = HideFlags.DontSaveInEditor };
            wrapper.transform.SetParent(_container.transform, false);
            wrapper.layer = _root.gameObject.layer;
            // 用户 GO 挂 wrapper。
            go.transform.SetParent(wrapper.transform, false);
            SetLayerRecursive(go, _root.gameObject.layer);
            // 材质 Transparent 配置由 caller 显式调 ConfigureTransparentMaterials（不在此自动碰——
            // 材质归 caller，框架不接管 GO 生命周期/派生资产 ownership）。
            _bindings[nodeId] = go;
            _wrappers[nodeId] = wrapper;
        }

        static void SetLayerRecursive(GameObject go, int layer)
        {
            go.layer = layer;
            foreach (Transform t in go.GetComponentsInChildren<Transform>(true))
                t.gameObject.layer = layer;
        }

        /// clone 标记：Configure 给 clone 材质 name 追加此后缀，Unconfigure 据此识别销毁。
        const string CLONE_SUFFIX = " (LoomNH)";

        /// 遍历 go 子树 Mesh/SkinnedMesh/Particle Renderer：clone sharedMaterial + 设 URP Transparent
        /// （坑 129：renderQueue=3000 + _Surface=1 + _SURFACE_TYPE_TRANSPARENT keyword + _ZWrite=0），
        /// 挂回 renderer.material（instance，不污染 sharedMaterial 资产）。clone 的 name 追加 CLONE_SUFFIX。
        /// 幂等：renderer.material 已带后缀则跳过（重复调不叠加 clone）。caller 在 Instantiate 后调一次。
        /// GO 须为 prefab 实例（caller 保证 sharedMaterial 是资产引用，clone 不影响其他实例）。
        public static void ConfigureTransparentMaterials(GameObject go)
        {
            if (go == null) return;
            foreach (var r in go.GetComponentsInChildren<Renderer>(true))
            {
                if (r == null) continue;
                // ParticleSystemRenderer 同纳入：粒子队列须与 UI 一致（3000），否则 sortingOrder 跨 UI/GO 错乱。
                if (!(r is MeshRenderer || r is SkinnedMeshRenderer || r is ParticleSystemRenderer))
                    continue;
                var src = r.sharedMaterials;
                if (src.Length == 0) continue;
                // 幂等：首个 material 已带后缀 → 已配过，跳过（假设同 renderer 所有槽一起配）。
                if (src[0] != null && src[0].name.EndsWith(CLONE_SUFFIX, System.StringComparison.Ordinal))
                    continue;
                var cloned = new Material[src.Length];
                for (int i = 0; i < src.Length; i++)
                {
                    var m = src[i];
                    if (m == null) { cloned[i] = null; continue; }
                    var c = new Material(m) { name = m.name + CLONE_SUFFIX };
                    c.renderQueue = 3000;
                    c.SetInt("_Surface", 1);
                    c.EnableKeyword("_SURFACE_TYPE_TRANSPARENT");
                    c.SetInt("_ZWrite", 0);
                    cloned[i] = c;
                }
                r.materials = cloned;  // 设 instance materials（clone 不污染 sharedMaterial 资产）
            }
        }

        /// 遍历同一 go 子树，销毁 name 含 CLONE_SUFFIX 的 instance material（Configure 配的 clone）。
        /// 与 Configure 对称，传同一 go。caller 在销毁 GO 前调一次（之后 GO 即销毁，不还原 sharedMaterial）。
        public static void UnconfigureTransparentMaterials(GameObject go)
        {
            if (go == null) return;
            foreach (var r in go.GetComponentsInChildren<Renderer>(true))
            {
                if (r == null) continue;
                // 遍历 instance materials（r.materials getter 返回当前 instance，含 clone）。
                foreach (var m in r.materials)
                {
                    if (m == null) continue;
                    if (m.name.EndsWith(CLONE_SUFFIX, System.StringComparison.Ordinal))
                    {
                        if (Application.isPlaying) Object.Destroy(m);
                        else Object.DestroyImmediate(m);
                    }
                }
            }
        }

        public void Unbind(uint nodeId)
        {
            if (_bindings.TryGetValue(nodeId, out var go))
            {
                go.SetActive(false);
                // Reparent user GO off wrapper before destroying wrapper.
                // Unity Destroy 递归销毁子树——wrapper 的子（user GO）会被连带销毁，
                // 破坏 caller 的"跨 Unbind 复用同一 GO"预期（如 driver 缓存 _characterInstance）。
                go.transform.SetParent(_container.transform, false);
                _bindings.Remove(nodeId);
            }
            if (_wrappers.TryGetValue(nodeId, out var wrapper))
            {
                if (Application.isPlaying) Object.Destroy(wrapper);
                else Object.DestroyImmediate(wrapper);
                _wrappers.Remove(nodeId);
            }
        }

        public void Clear()
        {
            // 销毁 wrapper 前先把 user GO reparent 出来（同 Unbind）——Destroy 递归销毁子树，
            // 不 reparent 则 user GO（调用方跨页复用实例，如 driver 缓存 _characterInstance）被连带销毁。
            foreach (var kv in _bindings)
            {
                if (kv.Value != null)
                {
                    kv.Value.SetActive(false);
                    kv.Value.transform.SetParent(_container.transform, false);
                }
            }
            _bindings.Clear();
            foreach (var kv in _wrappers)
            {
                if (kv.Value != null)
                {
                    if (Application.isPlaying) Object.Destroy(kv.Value);
                    else Object.DestroyImmediate(kv.Value);
                }
            }
            _wrappers.Clear();
        }

        /// <summary>
        /// 每帧 MirrorPool.Sync 后调：遍历 _bindings，FFI 查每个 nodeId 的
        /// world_matrix/sort_key/visible，设 wrapper TRS + GO sortingOrder + SetActive。
        /// 用户 GO 自身 transform 不动。不再遍历 blob——空 div slot 被 merge_meshes
        /// 吞后 RenderNode 消失，但 world_transforms/sort_keys/visible 保留全节点，
        /// 故 FFI 查询仍可拿到（与 merge 解耦）。null/无效/RemoveNode/display:none → visible=0 → SetActive(false)。
        /// </summary>
        public unsafe void Sync(StageHandle* stage)
        {
            if (_bindings.Count == 0) return;
            if (stage == null) return;
            float sf = Mathf.Abs(_root.localScale.y);  // root (sf,-sf,sf) → 取 |y|

            foreach (var kv in _bindings)
            {
                uint id = kv.Key;
                var go = kv.Value;
                if (go == null) continue;

                // visible（含 RemoveNode / display:none / 无效 NodeId → 0）
                byte vis = 0;
                Native.loomgui_stage_get_node_visible(stage, id, &vis);
                if (vis == 0) { if (go.activeSelf) go.SetActive(false); continue; }

                // world_matrix：a,b,c,d,tx,ty（Affine2 列主序）
                float a = 0, b = 0, c = 0, d = 0, tx = 0, ty = 0;
                Native.loomgui_stage_get_node_world_matrix(stage, id, &a, &b, &c, &d, &tx, &ty);

                if (!_wrappers.TryGetValue(id, out var wrapper) || wrapper == null) continue;

                // TRS 分解（剪切 case 降级）
                float rot = Mathf.Atan2(b, a) * Mathf.Rad2Deg;
                float sx = Mathf.Sqrt(a * a + b * b);
                float sy = Mathf.Sqrt(c * c + d * d);
                // wrapper 挂 _container（localScale (1,-1,1)）。container.worldScale=(sf,sf,sf)。
                // wrapper.localPosition (tx, -ty, 0)：design y-down → local y 翻，container (1,-1,1) 再翻
                //   → world y = rootPos.y - sf·ty（与 UI mesh worldPos 一致）。
                // wrapper.localScale (sx, sy, 1/sf)：worldScale = (sx·sf, sy·sf, 1)（z 不压扁）。
                wrapper.transform.localPosition = new Vector3(tx, -ty, 0);
                wrapper.transform.localRotation = Quaternion.Euler(0, 0, rot);
                wrapper.transform.localScale = new Vector3(sx, sy, sf > 0.0001f ? 1.0f / sf : 1.0f);

                // sortingOrder = 宿主 sort_key + HostSortOrderLift：盖过 merge_meshes 重组后
                // 编号在宿主之上的本地合并块（失效边界见常量注释）。
                uint sk = 0;
                Native.loomgui_stage_get_node_sort_key(stage, id, &sk);
                // includeInactive=true：刚恢复显示那帧 go.activeSelf 仍为 false（下一行才 SetActive(true)），
                // 不含 inactive 子节点则其 Renderer sortingOrder 漏更新一帧。
                foreach (var r in go.GetComponentsInChildren<Renderer>(true))
                    if (r != null) r.sortingOrder = (int)sk + HostSortOrderLift;
                if (!go.activeSelf) go.SetActive(true);
            }
        }
    }
}
