// uloop MCP server 自启（AI 验收会话辅助）：编辑器加载后若 server 未运行则拉起——
// uloop launch 的自动启动在本机不稳定（超时后 server 不起，需手动 Window > Unity
// CLI Loop > Server）。服务器仅本机回环供 CLI Loop 连接，无副作用；不需要时可删本文件。
using UnityEditor;
using UnityEngine;

namespace Showcase.EditorTools
{
    [InitializeOnLoad]
    internal static class UloopAutoStartServer
    {
        static UloopAutoStartServer()
        {
            // 延迟到首次 editor update：InitializeOnLoad 时机早于 package 全就绪，
            // 立即 StartServer 可能撞上 domain reload。
            EditorApplication.delayCall += () =>
            {
                try
                {
                    var (running, port, _) = io.github.hatayama.uLoopMCP.McpServerController.GetServerStatus();
                    if (running)
                    {
                        Debug.Log($"[UloopAutoStart] server already running on port {port}");
                        return;
                    }
                    io.github.hatayama.uLoopMCP.McpServerController.StartServer();
                    Debug.Log("[UloopAutoStart] uloop MCP server start requested");
                }
                catch (System.Exception e)
                {
                    Debug.LogWarning($"[UloopAutoStart] failed: {e.Message}");
                }
            };
        }
    }
}
