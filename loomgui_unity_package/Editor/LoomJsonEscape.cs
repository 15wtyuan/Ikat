namespace LoomGUI.Editor
{
    /// <summary>
    /// JSON 字符串值转义（" \ 与控制字符）。手拼 JSON 必须转义每个插值，否则路径/包名
    /// 含 " 或 \ 会产非法 JSON。纯逻辑、无 Unity 依赖，可 dotnet 单测。
    /// </summary>
    public static class LoomJsonEscape
    {
        public static string Escape(string s)
        {
            if (string.IsNullOrEmpty(s)) return s ?? "";
            var sb = new System.Text.StringBuilder(s.Length + 2);
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': sb.Append("\\\""); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default:
                        if (c < 0x20) sb.Append("\\u" + ((int)c).ToString("X4"));
                        else sb.Append(c);
                        break;
                }
            }
            return sb.ToString();
        }
    }
}
