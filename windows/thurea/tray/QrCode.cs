// QR code encoder for the Thurea tray: byte mode, error correction level M, all versions.
// Port of crates/thurea-core/src/qr.rs (which follows Project Nayuki's QR Code generator, MIT).
using System;
using System.Collections.Generic;
using System.Drawing;

namespace Thurea
{
    class QrCode
    {
        static readonly int[] EccPerBlockM = { -1, 10, 16, 26, 18, 24, 16, 18, 22, 22, 26, 30, 22, 22, 24, 24, 28, 28, 26, 26, 26, 26, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28, 28 };
        static readonly int[] NumBlocksM = { -1, 1, 1, 1, 2, 2, 4, 4, 4, 5, 5, 5, 8, 9, 9, 10, 10, 11, 13, 14, 16, 17, 17, 18, 20, 21, 23, 25, 26, 28, 29, 31, 33, 35, 37, 38, 40, 43, 45, 47, 49 };
        const int FormatBitsM = 0;

        public int Version, Size;
        bool[] mod, fn;

        static int RawDataModules(int v)
        {
            int r = (16 * v + 128) * v + 64;
            if (v >= 2) { int n = v / 7 + 2; r -= (25 * n - 10) * n - 55; if (v >= 7) r -= 36; }
            return r;
        }
        static int DataCodewords(int v) { return RawDataModules(v) / 8 - EccPerBlockM[v] * NumBlocksM[v]; }

        static int GfMul(int x, int y)
        {
            int z = 0;
            for (int i = 7; i >= 0; i--) { z = (z << 1) ^ ((z >> 7) * 0x11D); z ^= ((y >> i) & 1) * x; }
            return z & 0xFF;
        }
        static int[] RsDivisor(int degree)
        {
            var r = new int[degree]; r[degree - 1] = 1; int root = 1;
            for (int i = 0; i < degree; i++)
            {
                for (int j = 0; j < degree; j++) { r[j] = GfMul(r[j], root); if (j + 1 < degree) r[j] ^= r[j + 1]; }
                root = GfMul(root, 2);
            }
            return r;
        }
        static int[] RsRemainder(List<int> data, int[] div)
        {
            var r = new int[div.Length];
            foreach (int b in data)
            {
                int f = b ^ r[0];
                Array.Copy(r, 1, r, 0, r.Length - 1); r[r.Length - 1] = 0;
                for (int i = 0; i < r.Length; i++) r[i] ^= GfMul(div[i], f);
            }
            return r;
        }

        public static QrCode Encode(byte[] data)
        {
            int ver = 0;
            for (int v = 1; v <= 40; v++)
            {
                int cb = v <= 9 ? 8 : 16;
                if (4 + cb + data.Length * 8 <= DataCodewords(v) * 8 && (v > 9 || data.Length < 256)) { ver = v; break; }
            }
            if (ver == 0) throw new ArgumentException("too long for a QR code");
            int cap = DataCodewords(ver) * 8;
            var bits = new List<bool>(cap);
            Action<int, int> push = (val, n) => { for (int i = n - 1; i >= 0; i--) bits.Add(((val >> i) & 1) == 1); };
            push(4, 4);
            push(data.Length, ver <= 9 ? 8 : 16);
            foreach (byte b in data) push(b, 8);
            push(0, Math.Min(4, cap - bits.Count));
            push(0, (8 - bits.Count % 8) % 8);
            var words = new List<int>();
            for (int i = 0; i < bits.Count; i += 8) { int w = 0; for (int j = 0; j < 8; j++) w = (w << 1) | (bits[i + j] ? 1 : 0); words.Add(w); }
            for (bool alt = true; words.Count < cap / 8; alt = !alt) words.Add(alt ? 0xEC : 0x11);
            var all = AddEcc(words, ver);

            var q = new QrCode { Version = ver, Size = ver * 4 + 17 };
            q.mod = new bool[q.Size * q.Size]; q.fn = new bool[q.Size * q.Size];
            q.DrawFunctionPatterns();
            q.DrawCodewords(all);
            int best = 0; long bestP = long.MaxValue;
            for (int m = 0; m < 8; m++)
            {
                q.ApplyMask(m); q.DrawFormat(m);
                long p = q.Penalty();
                if (p < bestP) { bestP = p; best = m; }
                q.ApplyMask(m);
            }
            q.ApplyMask(best); q.DrawFormat(best);
            return q;
        }

        static List<int> AddEcc(List<int> data, int ver)
        {
            int nb = NumBlocksM[ver], el = EccPerBlockM[ver], raw = RawDataModules(ver) / 8;
            int nshort = nb - raw % nb, shortLen = raw / nb;
            var div = RsDivisor(el);
            var blocks = new List<List<int>>();
            int k = 0;
            for (int i = 0; i < nb; i++)
            {
                int n = shortLen - el + (i < nshort ? 0 : 1);
                var b = data.GetRange(k, n); k += n;
                var e = RsRemainder(b, div);
                if (i < nshort) b.Add(0);
                b.AddRange(e);
                blocks.Add(b);
            }
            var outp = new List<int>(raw);
            for (int i = 0; i < blocks[0].Count; i++)
                for (int j = 0; j < blocks.Count; j++)
                    if (i != shortLen - el || j >= nshort) outp.Add(blocks[j][i]);
            return outp;
        }

        public bool Get(int x, int y) { return mod[y * Size + x]; }
        void SetFn(int x, int y, bool d) { mod[y * Size + x] = d; fn[y * Size + x] = true; }

        void DrawFunctionPatterns()
        {
            int s = Size;
            for (int i = 0; i < s; i++) { SetFn(6, i, i % 2 == 0); SetFn(i, 6, i % 2 == 0); }
            foreach (var c in new[] { new[] { 3, 3 }, new[] { s - 4, 3 }, new[] { 3, s - 4 } })
                for (int dy = -4; dy <= 4; dy++)
                    for (int dx = -4; dx <= 4; dx++)
                    {
                        int x = c[0] + dx, y = c[1] + dy;
                        if (x >= 0 && y >= 0 && x < s && y < s) { int d = Math.Max(Math.Abs(dx), Math.Abs(dy)); SetFn(x, y, d != 2 && d != 4); }
                    }
            var pos = AlignmentPositions();
            int n = pos.Count;
            for (int i = 0; i < n; i++)
                for (int j = 0; j < n; j++)
                {
                    if ((i == 0 && j == 0) || (i == 0 && j == n - 1) || (i == n - 1 && j == 0)) continue;
                    for (int dy = -2; dy <= 2; dy++)
                        for (int dx = -2; dx <= 2; dx++)
                            SetFn(pos[i] + dx, pos[j] + dy, Math.Max(Math.Abs(dx), Math.Abs(dy)) != 1);
                }
            DrawFormat(0);
            if (Version >= 7)
            {
                int rem = Version;
                for (int i = 0; i < 12; i++) rem = (rem << 1) ^ ((rem >> 11) * 0x1F25);
                int bits = (Version << 12) | rem;
                for (int i = 0; i < 18; i++)
                {
                    bool d = ((bits >> i) & 1) == 1;
                    int a = s - 11 + i % 3, b = i / 3;
                    SetFn(a, b, d); SetFn(b, a, d);
                }
            }
        }

        List<int> AlignmentPositions()
        {
            var r = new List<int>();
            if (Version == 1) return r;
            int n = Version / 7 + 2;
            int step = (Version * 8 + n * 3 + 5) / (n * 4 - 4) * 2;
            for (int i = 0; i < n - 1; i++) r.Add(Size - 7 - i * step);
            r.Add(6);
            r.Reverse();
            return r;
        }

        void DrawFormat(int mask)
        {
            int data = (FormatBitsM << 3) | mask, rem = data;
            for (int i = 0; i < 10; i++) rem = (rem << 1) ^ ((rem >> 9) * 0x537);
            int bits = ((data << 10) | rem) ^ 0x5412;
            Func<int, bool> bit = i => ((bits >> i) & 1) == 1;
            int s = Size;
            for (int i = 0; i <= 5; i++) SetFn(8, i, bit(i));
            SetFn(8, 7, bit(6)); SetFn(8, 8, bit(7)); SetFn(7, 8, bit(8));
            for (int i = 9; i < 15; i++) SetFn(14 - i, 8, bit(i));
            for (int i = 0; i < 8; i++) SetFn(s - 1 - i, 8, bit(i));
            for (int i = 8; i < 15; i++) SetFn(8, s - 15 + i, bit(i));
            SetFn(8, s - 8, true);
        }

        void DrawCodewords(List<int> data)
        {
            int s = Size, i = 0;
            for (int right = s - 1; right >= 1; right -= 2)
            {
                if (right == 6) right = 5;
                for (int vert = 0; vert < s; vert++)
                    for (int j = 0; j < 2; j++)
                    {
                        int x = right - j;
                        bool up = ((right + 1) & 2) == 0;
                        int y = up ? s - 1 - vert : vert;
                        if (!fn[y * s + x] && i < data.Count * 8) { mod[y * s + x] = ((data[i >> 3] >> (7 - (i & 7))) & 1) == 1; i++; }
                    }
            }
        }

        void ApplyMask(int m)
        {
            int s = Size;
            for (int y = 0; y < s; y++)
                for (int x = 0; x < s; x++)
                {
                    bool inv;
                    switch (m)
                    {
                        case 0: inv = (x + y) % 2 == 0; break;
                        case 1: inv = y % 2 == 0; break;
                        case 2: inv = x % 3 == 0; break;
                        case 3: inv = (x + y) % 3 == 0; break;
                        case 4: inv = (x / 3 + y / 2) % 2 == 0; break;
                        case 5: inv = x * y % 2 + x * y % 3 == 0; break;
                        case 6: inv = (x * y % 2 + x * y % 3) % 2 == 0; break;
                        default: inv = ((x + y) % 2 + x * y % 3) % 2 == 0; break;
                    }
                    if (inv && !fn[y * s + x]) mod[y * s + x] = !mod[y * s + x];
                }
        }

        long Penalty()
        {
            int s = Size; long p = 0;
            for (int pass = 0; pass < 2; pass++)
                for (int a = 0; a < s; a++)
                {
                    Func<int, bool> get = i => pass == 0 ? Get(i, a) : Get(a, i);
                    int run = 1;
                    for (int i = 1; i < s; i++)
                    {
                        if (get(i) == get(i - 1)) { run++; if (run == 5) p += 3; else if (run > 5) p += 1; }
                        else run = 1;
                    }
                    Func<int, bool> at = i => i >= 0 && i < s && get(i);
                    bool[] core = { true, false, true, true, true, false, true };
                    for (int i = -4; i < s; i++)
                    {
                        bool match = true;
                        for (int k = 0; k < 7 && match; k++) if (at(i + k) != core[k]) match = false;
                        if (!match) continue;
                        bool before = true, after = true;
                        for (int k = 1; k <= 4; k++) if (at(i - k)) before = false;
                        for (int k = 7; k < 11; k++) if (at(i + k)) after = false;
                        if (before || after) p += 40;
                    }
                }
            for (int y = 0; y < s - 1; y++)
                for (int x = 0; x < s - 1; x++)
                {
                    bool c = Get(x, y);
                    if (c == Get(x + 1, y) && c == Get(x, y + 1) && c == Get(x + 1, y + 1)) p += 3;
                }
            int dark = 0; foreach (bool d in mod) if (d) dark++;
            int total = s * s;
            int k2 = (Math.Abs(dark * 20 - total * 10) + total - 1) / total;
            return p + Math.Max(0, k2 - 1) * 10;
        }

        public Bitmap ToBitmap(int scale)
        {
            int n = (Size + 8) * scale;
            var bmp = new Bitmap(n, n);
            using (var g = Graphics.FromImage(bmp))
            {
                g.Clear(Color.White);
                for (int y = 0; y < Size; y++)
                    for (int x = 0; x < Size; x++)
                        if (Get(x, y)) g.FillRectangle(Brushes.Black, (x + 4) * scale, (y + 4) * scale, scale, scale);
            }
            return bmp;
        }
    }
}
