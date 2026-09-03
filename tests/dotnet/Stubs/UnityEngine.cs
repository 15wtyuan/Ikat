namespace UnityEngine
{
    public struct Vector2
    {
        public float x;
        public float y;
        public Vector2(float x, float y) { this.x = x; this.y = y; }
    }

    public struct Color
    {
        public float r;
        public float g;
        public float b;
        public float a;
        public Color(float r, float g, float b, float a) { this.r = r; this.g = g; this.b = b; this.a = a; }
    }

    public struct Vector4
    {
        public float x;
        public float y;
        public float z;
        public float w;
        public Vector4(float x, float y, float z, float w) { this.x = x; this.y = y; this.z = z; this.w = w; }
    }

    public static class Mathf
    {
        public static float Abs(float f) => System.Math.Abs(f);
        public static float Min(float a, float b) => System.Math.Min(a, b);
    }
}
