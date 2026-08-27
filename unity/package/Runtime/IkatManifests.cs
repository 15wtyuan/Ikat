using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace Ikat
{
    /// <summary>
    /// Backend bootstrap manifest. Mirrors the Rust RuntimeManifest produced by the packer
    /// (ikat.runtime.json). Lists packages, atlases, and fonts the backend needs to load at startup.
    /// </summary>
    public class RuntimeManifest
    {
        public int version;
        public List<string> packages = new List<string>();
        public List<string> atlases = new List<string>();
        public List<RuntimeFont> fonts = new List<RuntimeFont>();
        /// 设计分辨率（分辨率适配正主，workspace.design 打包透传）。null = 集成层 fallback（Driver Inspector）。
        public DesignDim design;
        /// 适配模式 letterbox | fit-width | fit-height（workspace.match_mode 打包透传）。null = letterbox。
        public string match_mode;

        /// <summary>Parse ikat.runtime.json content.</summary>
        public static RuntimeManifest ParseRuntime(string json)
        {
            var r = new Reader(json);
            var m = new RuntimeManifest();
            r.ExpectObject();
            while (!r.TryObjectEnd())
            {
                switch (r.ReadKey())
                {
                    case "version":  m.version = r.ReadInt32(); break;
                    case "packages": r.ReadStringList(m.packages); break;
                    case "atlases":  r.ReadStringList(m.atlases); break;
                    case "fonts":
                        r.ExpectArray();
                        while (!r.TryArrayEnd())
                            m.fonts.Add(RuntimeFont.Read(r));
                        break;
                    case "design":
                        r.ExpectObject();
                        var d = new DesignDim();
                        while (!r.TryObjectEnd())
                        {
                            switch (r.ReadKey())
                            {
                                case "w": d.w = r.ReadFloat(); break;
                                case "h": d.h = r.ReadFloat(); break;
                                default: r.SkipValue(); break;
                            }
                        }
                        m.design = d;
                        break;
                    case "match_mode": m.match_mode = r.ReadString(); break;
                    default: r.SkipValue(); break;
                }
            }
            return m;
        }
    }

    /// <summary>Per-font entry in the runtime manifest.</summary>
    /// 设计分辨率（w,h，design px）。workspace.design 的 manifest 投影（Rust DesignDim 镜像）。
    public class DesignDim
    {
        public float w;
        public float h;
    }

    public class RuntimeFont
    {
        public string family;
        public string file;
        public bool @default;
        public bool fallback;

        internal static RuntimeFont Read(Reader r)
        {
            var f = new RuntimeFont();
            r.ExpectObject();
            while (!r.TryObjectEnd())
            {
                switch (r.ReadKey())
                {
                    case "family":   f.family   = r.ReadString(); break;
                    case "file":     f.file     = r.ReadString(); break;
                    case "default":  f.@default = r.ReadBool(); break;
                    case "fallback": f.fallback = r.ReadBool(); break;
                    default: r.SkipValue(); break;
                }
            }
            return f;
        }
    }

    /// <summary>
    /// Per-atlas sprite lookup table. Mirrors the Rust AtlasManifest (&lt;name&gt;.atlas.json).
    /// Maps sprite_key (workspace-relative path) to its UV rect and original pixel size.
    /// </summary>
    public class AtlasManifest
    {
        /// <summary>Page PNG filenames (e.g. ["ui.png"]). Index corresponds to SpriteEntry.page.</summary>
        public List<string> pages = new List<string>();

        /// <summary>sprite_key (e.g. "assets/icons/home.png") to its atlas entry.</summary>
        public Dictionary<string, SpriteEntry> sprites = new Dictionary<string, SpriteEntry>();

        /// <summary>Parse &lt;name&gt;.atlas.json content.</summary>
        public static AtlasManifest ParseAtlas(string json)
        {
            var r = new Reader(json);
            var m = new AtlasManifest();
            r.ExpectObject();
            while (!r.TryObjectEnd())
            {
                switch (r.ReadKey())
                {
                    case "pages": r.ReadStringList(m.pages); break;
                    case "sprites":
                        r.ExpectObject();
                        while (!r.TryObjectEnd())
                        {
                            var key = r.ReadKey();
                            m.sprites[key] = SpriteEntry.Read(r);
                        }
                        break;
                    default: r.SkipValue(); break;
                }
            }
            return m;
        }
    }

    /// <summary>
    /// One sprite entry in an atlas: page index, normalized UV rect, original pixel size.
    /// Mirrors the Rust SpriteEntry produced by the packer.
    /// </summary>
    public struct SpriteEntry
    {
        /// <summary>Index into AtlasManifest.pages.</summary>
        public int page;

        /// <summary>Normalized UV [u0, v0, u1, v1].</summary>
        public float[] uv;

        /// <summary>Original pixel size [w, h].</summary>
        public int[] orig;

        internal static SpriteEntry Read(Reader r)
        {
            var e = new SpriteEntry();
            r.ExpectObject();
            while (!r.TryObjectEnd())
            {
                switch (r.ReadKey())
                {
                    case "page": e.page = r.ReadInt32(); break;
                    case "uv":   e.uv   = r.ReadFloatArray(4); break;
                    case "orig": e.orig = r.ReadInt32Array(2); break;
                    default: r.SkipValue(); break;
                }
            }
            return e;
        }
    }

    // The JSON files parsed here are produced by Rust serde_json in a known,
    // stable format. This reader handles the subset we need: objects, arrays,
    // strings, numbers, booleans, null. It is intentionally not a general-purpose
    // JSON parser — it exists to avoid pulling in a JSON library dependency.

    /// <summary>
    /// Incremental JSON reader for the two manifest formats (ikat.runtime.json and atlas.json).
    /// Throws FormatException with position info on unexpected input.
    /// </summary>
    internal class Reader
    {
        private readonly string _json;
        private int _pos;

        public Reader(string json)
        {
            _json = json ?? throw new ArgumentNullException(nameof(json));
            _pos = 0;
        }

        private void SkipWS()
        {
            while (_pos < _json.Length && char.IsWhiteSpace(_json[_pos]))
                _pos++;
        }

        private char Peek()
        {
            SkipWS();
            if (_pos >= _json.Length)
                throw Fail("unexpected end of JSON");
            return _json[_pos];
        }

        public void Expect(char c)
        {
            SkipWS();
            if (_pos >= _json.Length || _json[_pos] != c)
                throw Fail($"expected '{c}'");
            _pos++;
        }

        public void ExpectObject() => Expect('{');
        public void ExpectArray()  => Expect('[');

        /// <summary>
        /// Consume an optional element-separator comma, then check for '}'.
        /// Returns true and consumes '}' if the object ends. Handles the
        /// comma between key-value pairs so callers can use a simple while loop.
        /// </summary>
        public bool TryObjectEnd()
        {
            SkipWS();
            if (_pos < _json.Length && _json[_pos] == ',')
            {
                _pos++;
                SkipWS();
            }
            if (_pos < _json.Length && _json[_pos] == '}')
            {
                _pos++;
                return true;
            }
            return false;
        }

        /// <summary>
        /// Consume an optional element-separator comma, then check for ']'.
        /// Returns true and consumes ']' if the array ends. Handles the
        /// comma between elements so callers can use a simple while loop.
        /// </summary>
        public bool TryArrayEnd()
        {
            SkipWS();
            if (_pos < _json.Length && _json[_pos] == ',')
            {
                _pos++;
                SkipWS();
            }
            if (_pos < _json.Length && _json[_pos] == ']')
            {
                _pos++;
                return true;
            }
            return false;
        }

        /// <summary>Read a JSON string key followed by ':'.</summary>
        public string ReadKey()
        {
            var key = ReadString();
            Expect(':');
            return key;
        }

        public string ReadString()
        {
            Expect('"');
            var sb = new StringBuilder();
            while (_pos < _json.Length)
            {
                char c = _json[_pos++];
                if (c == '"')
                    return sb.ToString();
                if (c == '\\')
                {
                    if (_pos >= _json.Length)
                        throw Fail("unterminated string escape");
                    char esc = _json[_pos++];
                    switch (esc)
                    {
                        case '"':  sb.Append('"'); break;
                        case '\\': sb.Append('\\'); break;
                        case '/':  sb.Append('/'); break;
                        case 'n':  sb.Append('\n'); break;
                        case 'r':  sb.Append('\r'); break;
                        case 't':  sb.Append('\t'); break;
                        case 'u':
                            if (_pos + 4 > _json.Length)
                                throw Fail("unterminated \\u escape");
                            var hex = _json.Substring(_pos, 4);
                            _pos += 4;
                            sb.Append((char)Convert.ToInt32(hex, 16));
                            break;
                        default:
                            // unknown escape — keep literal (serde should not produce these)
                            sb.Append('\\');
                            sb.Append(esc);
                            break;
                    }
                }
                else
                {
                    sb.Append(c);
                }
            }
            throw Fail("unterminated string");
        }

        public int ReadInt32()
        {
            SkipWS();
            int start = _pos;
            if (_pos < _json.Length && _json[_pos] == '-')
                _pos++;
            while (_pos < _json.Length && char.IsDigit(_json[_pos]))
                _pos++;
            if (_pos == start)
                throw Fail("expected integer");
            return int.Parse(_json.Substring(start, _pos - start), CultureInfo.InvariantCulture);
        }

        /// <summary>Read a JSON number as float (handles sign, decimal, scientific notation).</summary>
        public float ReadFloat()
        {
            SkipWS();
            int start = _pos;
            if (_pos < _json.Length && (_json[_pos] == '-' || _json[_pos] == '+'))
                _pos++;
            while (_pos < _json.Length && (char.IsDigit(_json[_pos]) || _json[_pos] == '.'))
                _pos++;
            if (_pos < _json.Length && (_json[_pos] == 'e' || _json[_pos] == 'E'))
            {
                _pos++;
                if (_pos < _json.Length && (_json[_pos] == '-' || _json[_pos] == '+'))
                    _pos++;
                while (_pos < _json.Length && char.IsDigit(_json[_pos]))
                    _pos++;
            }
            if (_pos == start)
                throw Fail("expected number");
            return float.Parse(
                _json.Substring(start, _pos - start),
                NumberStyles.Float,
                CultureInfo.InvariantCulture);
        }

        public bool ReadBool()
        {
            SkipWS();
            if (_pos + 4 <= _json.Length && _json.Substring(_pos, 4) == "true")
            {
                _pos += 4;
                return true;
            }
            if (_pos + 5 <= _json.Length && _json.Substring(_pos, 5) == "false")
            {
                _pos += 5;
                return false;
            }
            throw Fail("expected true or false");
        }

        public void ReadStringList(List<string> list)
        {
            ExpectArray();
            while (!TryArrayEnd())
                list.Add(ReadString());
        }

        public float[] ReadFloatArray(int expectedLen)
        {
            ExpectArray();
            var arr = new float[expectedLen];
            for (int i = 0; i < expectedLen; i++)
            {
                arr[i] = ReadFloat();
                if (i < expectedLen - 1)
                    Expect(',');
            }
            // trailing comma already consumed by loop; next must be ']'
            Expect(']');
            return arr;
        }

        public int[] ReadInt32Array(int expectedLen)
        {
            ExpectArray();
            var arr = new int[expectedLen];
            for (int i = 0; i < expectedLen; i++)
            {
                arr[i] = ReadInt32();
                if (i < expectedLen - 1)
                    Expect(',');
            }
            Expect(']');
            return arr;
        }

        /// <summary>Skip any JSON value (used for unknown keys).</summary>
        public void SkipValue()
        {
            SkipWS();
            if (_pos >= _json.Length)
                return;
            char c = _json[_pos];
            switch (c)
            {
                case '"':
                    ReadString();
                    break;
                case '{':
                    _pos++;
                    SkipNested('{', '}');
                    break;
                case '[':
                    _pos++;
                    SkipNested('[', ']');
                    break;
                case 't':
                case 'f':
                    ReadBool();
                    break;
                case 'n':
                    if (_pos + 4 <= _json.Length && _json.Substring(_pos, 4) == "null")
                        _pos += 4;
                    else
                        throw Fail("expected null");
                    break;
                default:
                    // number
                    if (c == '-') _pos++;
                    while (_pos < _json.Length && (char.IsDigit(_json[_pos]) || _json[_pos] == '.' || _json[_pos] == 'e' || _json[_pos] == 'E' || _json[_pos] == '+' || _json[_pos] == '-'))
                        _pos++;
                    break;
            }
        }

        private void SkipNested(char open, char close)
        {
            int depth = 1;
            bool inString = false;
            while (_pos < _json.Length && depth > 0)
            {
                char c = _json[_pos++];
                if (inString)
                {
                    if (c == '\\') { _pos++; continue; }
                    if (c == '"') inString = false;
                }
                else
                {
                    if (c == '"') inString = true;
                    else if (c == open) depth++;
                    else if (c == close) depth--;
                }
            }
        }

        private FormatException Fail(string msg)
        {
            int line = 1, col = 1;
            for (int i = 0; i < _pos && i < _json.Length; i++)
            {
                if (_json[i] == '\n') { line++; col = 1; }
                else col++;
            }
            int ctxStart = Math.Max(0, _pos - 20);
            int ctxLen = Math.Min(40, _json.Length - ctxStart);
            var ctx = _json.Substring(ctxStart, ctxLen).Replace("\n", "\\n").Replace("\r", "\\r");
            return new FormatException(
                $"JSON parse error at line {line} col {col} (pos {_pos}): {msg}. Context: \"{ctx}\"");
        }
    }
}
