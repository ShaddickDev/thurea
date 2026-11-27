// Thurea tray app for Windows (C# 5 / .NET Framework 4.8, compiled on the PC by Windows' own
// csc.exe, so nothing extra is installed). The Windows counterpart of Thurea's Omarchy plugin
// and Linux tray:
//   * the tray icon shows whether this PC is protected (C:\ProgramData\Thurea\status.json,
//     written every 5 minutes by the "Thurea" task);
//   * the Thurea window: the logo draws itself, the verse of the day (World English Bible,
//     public domain), protection status, settings and credits;
//   * "Set up a phone": the Android setup QR code. A new or factory-reset phone scans it on its
//     Welcome screen, downloads Thurea from this PC (checked by SHA-256) and makes Thurea its
//     device owner. The download exists only while the window is open, at a random address;
//   * "Get Thurea on another device": a page on this PC offering Thurea for Android / Windows.
// Settings are shared with Thurea's other desktop apps: %APPDATA%\Thurea\prefs.json.
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.NetworkInformation;
using System.Net.Sockets;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace Thurea
{
    static class Program
    {
        [STAThread]
        static void Main(string[] args)
        {
            // Build check: ThureaTray.exe --qr-test TEXT OUT.png writes the QR code for TEXT.
            if (args.Length == 3 && args[0] == "--qr-test")
            {
                QrCode.Encode(Encoding.UTF8.GetBytes(args[1])).ToBitmap(4).Save(args[2], System.Drawing.Imaging.ImageFormat.Png);
                return;
            }
            // Build check: ThureaTray.exe --icon-test OUT.png draws the logo as the tray does.
            if (args.Length == 2 && args[0] == "--icon-test")
            {
                using (var b = Logo.Bitmap(256, 1f, false)) b.Save(args[1], System.Drawing.Imaging.ImageFormat.Png);
                return;
            }
            bool first;
            using (var m = new Mutex(true, "Local\\ThureaTray", out first))
            {
                bool keep = args.Contains("--keep-phone");
                if (!first)
                {
                    // Already running: ask it to open its window (unless this is the login start).
                    if (keep) TrayContext.Signal(true);
                    else if (!args.Contains("--startup")) TrayContext.Signal(false);
                    return;
                }
                Application.EnableVisualStyles();
                Application.SetCompatibleTextRenderingDefault(false);
                var ctx = new TrayContext(!args.Contains("--startup") && !keep);
                if (keep) ctx.ShowPhone(true);
                Application.Run(ctx);
            }
        }
    }

    // ── Shared data: status, verse of the day, settings ──

    static class Paths
    {
        public static readonly string Home = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles), "Thurea");
        public static readonly string Data = Path.Combine(Home, "data");
        public static readonly string StatusFile = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.CommonApplicationData), "Thurea", "status.json");
        public static readonly string PrefsFile = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.ApplicationData), "Thurea", "prefs.json");
        public static readonly string Apk = Path.Combine(Home, "android", "Thurea.apk");
        public static readonly string WinZip = Path.Combine(Home, "windows", "thurea-windows.zip");
        public static readonly string Credits = Path.Combine(Home, "CREDITS.md");
        public static readonly string License = Path.Combine(Home, "LICENSE");
    }

    static class Prefs
    {
        public static bool Get(string key)
        {
            try
            {
                var d = (Dictionary<string, object>)new JavaScriptSerializer().DeserializeObject(File.ReadAllText(Paths.PrefsFile));
                object v;
                if (d.TryGetValue(key, out v) && v is bool) return (bool)v;
            }
            catch (Exception) { }
            return true;   // verses and animation are on unless switched off
        }

        public static void Set(string key, bool on)
        {
            var d = new Dictionary<string, object>();
            try { d = (Dictionary<string, object>)new JavaScriptSerializer().DeserializeObject(File.ReadAllText(Paths.PrefsFile)); }
            catch (Exception) { }
            d[key] = on;
            Directory.CreateDirectory(Path.GetDirectoryName(Paths.PrefsFile));
            File.WriteAllText(Paths.PrefsFile, new JavaScriptSerializer().Serialize(d));
        }
    }

    static class Verses
    {
        // policy/verses.json (World English Bible, public domain): one verse per day, in order, the
        // same verse as on Thurea for Android and Linux.
        public static string[] Today()
        {
            try
            {
                var d = (Dictionary<string, object>)new JavaScriptSerializer().DeserializeObject(File.ReadAllText(Path.Combine(Paths.Data, "verses.json"), Encoding.UTF8));
                var list = (object[])d["verses"];
                long day = (long)(DateTime.Today - new DateTime(1970, 1, 1)).TotalDays;
                var v = (Dictionary<string, object>)list[(int)(((day % list.Length) + list.Length) % list.Length)];
                return new[] { (string)v["ref"], (string)v["text"], d.ContainsKey("abbreviation") ? (string)d["abbreviation"] : "WEB" };
            }
            catch (Exception) { return null; }
        }
    }

    class Status
    {
        public bool Ok, Known;
        public string Line = "Thurea: status unknown";
        public string Version = "";
        public DateTime At;

        public static Status Read()
        {
            var s = new Status();
            try
            {
                var d = (Dictionary<string, object>)new JavaScriptSerializer().DeserializeObject(File.ReadAllText(Paths.StatusFile).TrimStart('\uFEFF'));
                s.At = DateTime.Parse((string)d["at"]);
                if (d.ContainsKey("version") && d["version"] != null) s.Version = d["version"].ToString().Trim();
                object protectedValue;
                s.Ok = d.TryGetValue("protected", out protectedValue) && protectedValue is bool && (bool)protectedValue;
                s.Known = true;
                var age = DateTime.Now - s.At;
                if (age > TimeSpan.FromMinutes(15)) { s.Ok = false; s.Line = "Not checked for " + (int)age.TotalMinutes + " minutes"; }
                else s.Line = s.Ok ? "Your Device is protected by Thurea" : "Not fully protected: Thurea is putting it back";
            }
            catch (Exception) { s.Line = "Thurea isn't reporting (is it installed?)"; }
            return s;
        }
    }

    // ── Thurea's setup codes, dressed up: rounded modules on a white card, round-cornered finder
    //    eyes in Thurea blue. The "Keep my phone" code also has the logo in a clear centre: it has
    //    High error correction (30%) and the centre hides under 9% of it, so it still scans. The
    //    "Reset phone" code is dense, so nothing covers it. Same drawing as the tray page
    //    (qrStyled in page.html) and the Omarchy panel (keepQr in Panel.qml). ──

    static class QrArt
    {
        static GraphicsPath Round(float x, float y, float w, float h, float r)
        {
            var p = new GraphicsPath();
            float d = r * 2;
            p.AddArc(x, y, d, d, 180, 90);
            p.AddArc(x + w - d, y, d, d, 270, 90);
            p.AddArc(x + w - d, y + h - d, d, d, 0, 90);
            p.AddArc(x, y + h - d, d, d, 90, 90);
            p.CloseFigure();
            return p;
        }

        public static Bitmap Draw(QrCode qr, int u, Color outside, bool logo)
        {
            var rows = new List<string>();
            for (int y = 0; y < qr.Size; y++)
            {
                var r = new StringBuilder(qr.Size);
                for (int x = 0; x < qr.Size; x++) r.Append(qr.Get(x, y) ? '1' : '0');
                rows.Add(r.ToString());
            }
            return Draw(rows, u, outside, logo);
        }

        // raw: the module rows (a quiet zone around them is trimmed off), as `thurea adopt-phone
        // --tray` sends them; u: pixels per module; outside: the colour behind the card's corners.
        public static Bitmap Draw(List<string> raw, int u, Color outside, bool logo)
        {
            int k = 0;
            while (k < raw.Count / 2 && raw[k].IndexOf('1') < 0) k++;
            int n = raw.Count - 2 * k, q = 3, size = (n + 2 * q) * u;
            int c = logo ? (int)Math.Floor(n * 0.27 + 0.5) | 1 : 0, c0 = (n - c) / 2;
            var bmp = new Bitmap(size, size);
            using (var g = Graphics.FromImage(bmp))
            using (var dark = new SolidBrush(Color.FromArgb(0x14, 0x17, 0x1C)))
            using (var blue = new SolidBrush(Color.FromArgb(0x1D, 0x4A, 0x94)))
            {
                g.SmoothingMode = SmoothingMode.AntiAlias;
                g.Clear(outside);
                using (var card = Round(0, 0, size, size, 2.4f * u)) g.FillPath(Brushes.White, card);
                for (int y = 0; y < n; y++)
                    for (int x = 0; x < n; x++)
                    {
                        if (raw[y + k][x + k] != '1') continue;
                        bool eye = (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7);
                        bool clear = x >= c0 && x < c0 + c && y >= c0 && y < c0 + c;
                        if (eye || clear) continue;
                        using (var m = Round((x + q + 0.06f) * u, (y + q + 0.06f) * u, 0.88f * u, 0.88f * u, 0.3f * u)) g.FillPath(dark, m);
                    }
                foreach (var at in new[] { new Point(0, 0), new Point(n - 7, 0), new Point(0, n - 7) })
                {
                    float ex = (at.X + q) * u, ey = (at.Y + q) * u;
                    using (var ring = Round(ex, ey, 7 * u, 7 * u, 2.1f * u))
                    using (var hole = Round(ex + u, ey + u, 5 * u, 5 * u, 1.3f * u))
                    {
                        ring.AddPath(hole, false);
                        ring.FillMode = FillMode.Alternate;
                        g.FillPath(blue, ring);
                    }
                    using (var dot = Round(ex + 2 * u, ey + 2 * u, 3 * u, 3 * u, 0.9f * u)) g.FillPath(blue, dot);
                }
                if (logo)
                {
                    float side = (c - 1.4f) * u, at0 = (q + c0 + 0.7f) * u;
                    Logo.Draw(g, new RectangleF(at0, at0, side, side), 1f, false);
                }
            }
            return bmp;
        }
    }

    // ── The logo: drawn here, and drawn a little at a time for the opening animation ──

    static class Logo
    {
        // Thurea's outline (logo units: x 31..77, y 27..82), as extension/icons/thurea.svg.
        public static readonly PointF[] Outline = Build();
        public static readonly PointF[] Check = { new PointF(43.5f, 54.5f), new PointF(51f, 62f), new PointF(65f, 47f) };

        static PointF[] Build()
        {
            var p = new List<PointF> { new PointF(54, 27), new PointF(77, 35.5f), new PointF(77, 52) };
            Cubic(p, new PointF(77, 52), new PointF(77, 66.5f), new PointF(67.5f, 76.5f), new PointF(54, 82));
            Cubic(p, new PointF(54, 82), new PointF(40.5f, 76.5f), new PointF(31, 66.5f), new PointF(31, 52));
            p.Add(new PointF(31, 35.5f)); p.Add(new PointF(54, 27));
            return p.ToArray();
        }

        static void Cubic(List<PointF> o, PointF a, PointF b, PointF c, PointF d)
        {
            for (int i = 1; i <= 24; i++)
            {
                float t = i / 24f, u = 1 - t;
                o.Add(new PointF(u * u * u * a.X + 3 * u * u * t * b.X + 3 * u * t * t * c.X + t * t * t * d.X,
                                 u * u * u * a.Y + 3 * u * u * t * b.Y + 3 * u * t * t * c.Y + t * t * t * d.Y));
            }
        }

        static float Length(PointF[] p)
        {
            float l = 0;
            for (int i = 1; i < p.Length; i++) l += (float)Math.Sqrt(Math.Pow(p[i].X - p[i - 1].X, 2) + Math.Pow(p[i].Y - p[i - 1].Y, 2));
            return l;
        }

        // The first t (0..1) of a polyline, by length.
        static PointF[] Trim(PointF[] p, float t)
        {
            if (t >= 1) return p;
            float want = Length(p) * Math.Max(0, t);
            var o = new List<PointF> { p[0] };
            for (int i = 1; i < p.Length; i++)
            {
                float d = (float)Math.Sqrt(Math.Pow(p[i].X - p[i - 1].X, 2) + Math.Pow(p[i].Y - p[i - 1].Y, 2));
                if (d >= want) { if (d > 0) o.Add(new PointF(p[i - 1].X + (p[i].X - p[i - 1].X) * want / d, p[i - 1].Y + (p[i].Y - p[i - 1].Y) * want / d)); break; }
                want -= d; o.Add(p[i]);
            }
            return o.ToArray();
        }

        static float Clamp(float x) { return Math.Max(0, Math.Min(1, x)); }
        static float Ease(float x) { return 1 - (float)Math.Pow(1 - x, 3); }

        // Timeline of 1400 ms: outline 0-900, fill 750-1150, check 1050-1400 (as on Android and Linux).
        public static void Draw(Graphics g, RectangleF box, float progress, bool warning)
        {
            float ms = progress * 1400f;
            float outline = Ease(Clamp(ms / 900f)), fill = Clamp((ms - 750f) / 400f), check = Ease(Clamp((ms - 1050f) / 350f));
            g.SmoothingMode = SmoothingMode.AntiAlias;
            var state = g.Save();
            float s = Math.Min(box.Width, box.Height) / 58f;
            g.TranslateTransform(box.X + (box.Width - 56 * s) / 2 - 26 * s, box.Y + (box.Height - 56 * s) / 2 - 26.5f * s);
            g.ScaleTransform(s, s);
            Color main = warning ? Color.FromArgb(0xF2, 0x9B, 0x38) : Color.FromArgb(0x8C, 0xC8, 0xFF);
            if (fill > 0)
            {
                using (var path = new GraphicsPath())
                {
                    path.AddPolygon(Outline);
                    Brush b = warning ? (Brush)new SolidBrush(Color.FromArgb((int)(fill * 255), main))
                        : new LinearGradientBrush(new PointF(31, 27), new PointF(77, 82), Color.FromArgb((int)(fill * 255), 0xA9, 0xD8, 0xFF), Color.FromArgb((int)(fill * 255), 0x4F, 0x86, 0xE0));
                    using (b) g.FillPath(b, path);
                }
            }
            if (fill < 1)
            {
                var pts = Trim(Outline, outline);
                if (pts.Length > 1)
                    using (var pen = new Pen(main, 2.6f) { LineJoin = LineJoin.Round, StartCap = LineCap.Round, EndCap = LineCap.Round })
                        g.DrawLines(pen, pts);
            }
            if (check > 0)
            {
                var pts = Trim(Check, check);
                if (pts.Length > 1)
                    using (var pen = new Pen(Color.FromArgb(0x11, 0x13, 0x18), 5f) { LineJoin = LineJoin.Round, StartCap = LineCap.Round, EndCap = LineCap.Round })
                        g.DrawLines(pen, pts);
            }
            g.Restore(state);
        }

        public static Bitmap Bitmap(int size, float progress, bool warning)
        {
            var b = new Bitmap(size, size);
            using (var g = Graphics.FromImage(b)) { g.Clear(Color.Transparent); Draw(g, new RectangleF(0, 0, size, size), progress, warning); }
            return b;
        }

        public static Icon Icon(bool warning, bool unknown)
        {
            using (var b = Bitmap(64, 1f, warning))
            using (var o = new Bitmap(64, 64))
            {
                using (var g = Graphics.FromImage(o))
                using (var a = new System.Drawing.Imaging.ImageAttributes())
                {
                    // Not reporting: the logo in grey, half transparent.
                    float k = unknown ? 1f / 3 : 0;
                    var m = unknown
                        ? new System.Drawing.Imaging.ColorMatrix(new[] { new[] { k, k, k, 0, 0 }, new[] { k, k, k, 0, 0 }, new[] { k, k, k, 0, 0 }, new[] { 0, 0, 0, 0.55f, 0 }, new[] { 0, 0, 0, 0, 1f } })
                        : new System.Drawing.Imaging.ColorMatrix();
                    a.SetColorMatrix(m);
                    g.DrawImage(b, new Rectangle(0, 0, 64, 64), 0, 0, 64, 64, GraphicsUnit.Pixel, a);
                }
                IntPtr h = o.GetHicon();
                return (Icon)System.Drawing.Icon.FromHandle(h).Clone();
            }
        }
    }

    // A control that shows the logo, and draws it when Play() is called.
    class LogoView : Control
    {
        readonly System.Windows.Forms.Timer timer = new System.Windows.Forms.Timer { Interval = 15 };
        readonly Stopwatch clock = new Stopwatch();
        float progress = 1f;
        public bool Warning;

        public LogoView()
        {
            SetStyle(ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.UserPaint | ControlStyles.SupportsTransparentBackColor, true);
            BackColor = Color.Transparent;
            timer.Tick += delegate
            {
                progress = Math.Min(1f, clock.ElapsedMilliseconds / 1400f);
                if (progress >= 1f) timer.Stop();
                Invalidate();
            };
        }

        public void Play() { progress = 0; clock.Restart(); timer.Start(); Invalidate(); }

        protected override void OnPaint(PaintEventArgs e) { Logo.Draw(e.Graphics, new RectangleF(0, 0, Width, Height), progress, Warning); }

        protected override void Dispose(bool disposing) { if (disposing) timer.Dispose(); base.Dispose(disposing); }
    }

    // ── Colours of Thurea's windows (the same dark palette as the Android app) ──

    static class Theme
    {
        public static readonly Color Bg = Color.FromArgb(0x11, 0x13, 0x18), Card = Color.FromArgb(0x1B, 0x1E, 0x26), Line = Color.FromArgb(0x2A, 0x2F, 0x3B);
        public static readonly Color Text = Color.FromArgb(0xEC, 0xEE, 0xF2), Muted = Color.FromArgb(0x9A, 0xA0, 0xAA), Accent = Color.FromArgb(0x8C, 0xC8, 0xFF);
        public static readonly Color Good = Color.FromArgb(0x5D, 0xD3, 0x9E), Warn = Color.FromArgb(0xFF, 0xB4, 0x54);

        public static Label Label(string text, float size, Color color, FontStyle style = FontStyle.Regular)
        {
            return new Label { Text = text, AutoSize = false, ForeColor = color, BackColor = Color.Transparent, Font = new Font("Segoe UI", size, style), UseMnemonic = false };
        }

        public static Button Button(string text, bool primary)
        {
            var b = new Button { Text = text, FlatStyle = FlatStyle.Flat, Font = new Font("Segoe UI Semibold", 10f), Cursor = Cursors.Hand, Height = 38 };
            b.FlatAppearance.BorderColor = primary ? Accent : Line;
            b.FlatAppearance.BorderSize = 1;
            b.BackColor = primary ? Accent : Card;
            b.ForeColor = primary ? Bg : Text;
            return b;
        }

        public static CheckBox Check(string text, bool on)
        {
            return new CheckBox { Text = text, Checked = on, ForeColor = Text, BackColor = Color.Transparent, Font = new Font("Segoe UI", 10f), AutoSize = true, Cursor = Cursors.Hand };
        }
    }

    class ThureaForm : Form
    {
        enum WindowAttribute
        {
            UseImmersiveDarkMode = 20,
            CornerPreference = 33,
            BorderColor = 34,
            CaptionColor = 35,
            TextColor = 36
        }

        const int RoundCorners = 2;
        const int NoBorderColor = unchecked((int)0xFFFFFFFE);

        [DllImport("dwmapi.dll", ExactSpelling = true)]
        static extern int DwmSetWindowAttribute(IntPtr window, WindowAttribute attribute, ref int value, int size);

        protected ThureaForm()
        {
            BackColor = Theme.Bg;
            ForeColor = Theme.Text;
            Font = new Font("Segoe UI", 10f);
            FormBorderStyle = FormBorderStyle.FixedSingle;
            MaximizeBox = false;
            StartPosition = FormStartPosition.CenterScreen;
            using (var b = Logo.Bitmap(64, 1f, false)) Icon = System.Drawing.Icon.FromHandle(b.GetHicon());
        }

        protected override void OnHandleCreated(EventArgs e)
        {
            base.OnHandleCreated(e);
            if (SystemInformation.HighContrast) return;

            // Keep the native caption and window controls. Older Windows versions ignore any
            // attributes they do not support.
            SetWindowAttribute(WindowAttribute.UseImmersiveDarkMode, 1);
            SetWindowAttribute(WindowAttribute.CornerPreference, RoundCorners);
            SetWindowAttribute(WindowAttribute.BorderColor, NoBorderColor);
            SetWindowAttribute(WindowAttribute.CaptionColor, ColorTranslator.ToWin32(BackColor));
            SetWindowAttribute(WindowAttribute.TextColor, ColorTranslator.ToWin32(ForeColor));
        }

        void SetWindowAttribute(WindowAttribute attribute, int value)
        {
            DwmSetWindowAttribute(Handle, attribute, ref value, sizeof(int));
        }
    }

    // ── Tray icon ──

    class TrayContext : ApplicationContext
    {
        // Another ThureaTray.exe started by the user: open the window of the running one.
        static readonly string SignalName = "Local\\ThureaTrayOpen", KeepSignalName = "Local\\ThureaTrayKeepPhone";
        public static void Signal(bool keepPhone)
        {
            try { EventWaitHandle e; if (EventWaitHandle.TryOpenExisting(keepPhone ? KeepSignalName : SignalName, out e)) e.Set(); } catch (Exception) { }
        }

        readonly NotifyIcon icon = new NotifyIcon();
        readonly ToolStripMenuItem header = new ToolStripMenuItem("Thurea") { Enabled = false };
        readonly ToolStripMenuItem verseItem = new ToolStripMenuItem("") { Enabled = false };
        readonly System.Windows.Forms.Timer timer = new System.Windows.Forms.Timer();
        readonly EventWaitHandle openSignal = new EventWaitHandle(false, EventResetMode.AutoReset, SignalName);
        readonly EventWaitHandle keepSignal = new EventWaitHandle(false, EventResetMode.AutoReset, KeepSignalName);
        // Created on the UI thread with its window handle made at once, so the signal thread can
        // hand work to the UI thread (a SynchronizationContext made before the message loop
        // would run it on the calling thread, with no message loop, and the window would hang).
        readonly Control ui = new Control();
        bool? lastOk;
        MainForm main;
        PhoneForm phone;
        ShareForm share;

        public TrayContext(bool openWindow)
        {
            ui.CreateControl();
            IntPtr forceHandle = ui.Handle;
            var menu = new ContextMenuStrip();
            menu.Items.Add(header);
            menu.Items.Add(verseItem);
            menu.Items.Add(new ToolStripSeparator());
            menu.Items.Add("Open Thurea", null, delegate { ShowMain(); });
            menu.Items.Add("Family...", null, delegate { OpenFamily(); });
            menu.Items.Add("Set up a phone...", null, delegate { ShowPhone(); });
            menu.Items.Add("Get Thurea on another device...", null, delegate { ShowShare(); });
            menu.Items.Add("Re-apply protection now", null, delegate { Reapply(); });
            menu.Opening += delegate { Refresh(); };
            icon.ContextMenuStrip = menu;
            icon.Icon = Logo.Icon(false, true);
            icon.Text = "Thurea";
            icon.Visible = true;
            icon.MouseClick += (s, e) => { if (e.Button == MouseButtons.Left) ShowMain(); };
            timer.Interval = 30000;
            timer.Tick += delegate { Refresh(); };
            timer.Start();
            new Thread(() => { while (true) { openSignal.WaitOne(); try { ui.BeginInvoke((MethodInvoker)ShowMain); } catch (Exception) { } } }) { IsBackground = true }.Start();
            new Thread(() => { while (true) { keepSignal.WaitOne(); try { ui.BeginInvoke((MethodInvoker)delegate { ShowPhone(true); }); } catch (Exception) { } } }) { IsBackground = true }.Start();
            Refresh();
            if (openWindow) ShowMain();
        }

        void Refresh()
        {
            var s = Status.Read();
            string text = "Thurea: " + s.Line;
            header.Text = text;
            icon.Text = text.Length > 63 ? text.Substring(0, 63) : text;
            var old = icon.Icon;
            icon.Icon = Logo.Icon(!s.Ok && s.Known, !s.Known);
            if (old != null) old.Dispose();
            var v = Prefs.Get("verses") ? Verses.Today() : null;
            verseItem.Visible = v != null;
            if (v != null) { verseItem.Text = v[0] + " (" + v[2] + ")"; verseItem.ToolTipText = v[1]; }
            if (lastOk == true && !s.Ok) icon.ShowBalloonTip(10000, "Thurea", s.Line + ". Thurea puts it back within 5 minutes.", ToolTipIcon.Warning);
            lastOk = s.Ok;
            if (main != null && !main.IsDisposed) main.Render(s);
        }

        void ShowMain()
        {
            if (main == null || main.IsDisposed)
            {
                main = new MainForm(this);
                main.Show();
            }
            else { if (main.WindowState == FormWindowState.Minimized) main.WindowState = FormWindowState.Normal; main.Activate(); }
            main.Render(Status.Read());
        }

        /// The parents' control center (a page served by thurea.exe on this PC only, opened in the browser).
        /// `thurea control-center` returns within seconds (the page's server keeps running on its
        /// own): if it failed, say why; if no browser window could be opened, it printed the address.
        public void OpenFamily()
        {
            Process p;
            var output = new StringBuilder();
            try
            {
                p = new Process { StartInfo = new ProcessStartInfo(Path.Combine(Paths.Home, "thurea.exe"), "control-center")
                    { UseShellExecute = false, CreateNoWindow = true, RedirectStandardOutput = true, RedirectStandardError = true } };
                // Events, not ReadToEnd: the server it starts may hold the pipes open for a while.
                DataReceivedEventHandler collect = (s, e) => { if (e.Data != null) lock (output) output.AppendLine(e.Data); };
                p.OutputDataReceived += collect;
                p.ErrorDataReceived += collect;
                p.Start();
                p.BeginOutputReadLine();
                p.BeginErrorReadLine();
            }
            catch (Exception e) { MessageBox.Show("Couldn't open the family control center: " + e.Message, "Thurea"); return; }
            new Thread(() =>
            {
                try
                {
                    if (!p.WaitForExit(30000)) return;
                    Thread.Sleep(300);
                    string text; lock (output) text = output.ToString().Trim();
                    int code = p.ExitCode;
                    ui.BeginInvoke((MethodInvoker)delegate
                    {
                        if (code != 0)
                        {
                            MessageBox.Show("Couldn't open the family control center:\n\n" + text.Replace("thurea: ", ""), "Thurea", MessageBoxButtons.OK, MessageBoxIcon.Warning);
                            return;
                        }
                        int at = text.IndexOf("http://127.0.0.1:");
                        if (at < 0) return;
                        string url = text.Substring(at).Split(new[] { ' ', '\r', '\n' }, StringSplitOptions.RemoveEmptyEntries)[0];
                        try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); }
                        catch (Exception) { MessageBox.Show("Open this address in a browser on this PC:\n\n" + url, "Thurea family"); }
                    });
                }
                catch (Exception) { }
            }) { IsBackground = true }.Start();
        }

        // "Keep my phone" (set up a phone in use, without a reset) is the first choice.
        public void ShowPhone() { ShowPhone(true); }

        public void ShowPhone(bool keep)
        {
            if (phone != null && !phone.IsDisposed) { if (keep) phone.SetMode(true); phone.Activate(); return; }
            phone = new PhoneForm();
            phone.SetMode(keep);
            phone.Show();
        }

        public void ShowShare()
        {
            if (share != null && !share.IsDisposed) { share.Activate(); return; }
            share = new ShareForm();
            share.Show();
        }

        public void Reapply()
        {
            try
            {
                var p = new ProcessStartInfo("schtasks.exe", "/run /tn \"\\Thurea\\Thurea\"") { Verb = "runas", UseShellExecute = true, WindowStyle = ProcessWindowStyle.Hidden };
                Process.Start(p).WaitForExit();
                icon.ShowBalloonTip(5000, "Thurea", "Re-applying protection...", ToolTipIcon.Info);
                var t = new System.Windows.Forms.Timer { Interval = 20000 };
                t.Tick += delegate { t.Stop(); t.Dispose(); Refresh(); };
                t.Start();
            }
            catch (Exception) { }   // the user said no to Administrator: the task runs by itself every 5 minutes
        }
    }

    // ── The Thurea window ──

    class MainForm : ThureaForm
    {
        readonly TrayContext tray;
        readonly LogoView logo = new LogoView();
        readonly Label line = Theme.Label("", 10.5f, Theme.Muted);
        readonly Label version = Theme.Label("", 9f, Theme.Muted);
        readonly Panel verseCard = new Panel();
        readonly Label verseText = Theme.Label("", 12f, Theme.Text, FontStyle.Italic);
        readonly Label verseRef = Theme.Label("", 9f, Theme.Muted);
        readonly Label about = Theme.Label("", 10f, Theme.Text);
        readonly FlowLayoutPanel home = CreatePage();
        readonly FlowLayoutPanel settings = CreatePage();
        readonly ToolTip hints = new ToolTip();
        readonly Button gear, back;
        const int W = 440;

        public MainForm(TrayContext tray)
        {
            this.tray = tray;
            Text = "Thurea";
            ClientSize = new Size(W + 48, 300);
            settings.Visible = false;
            Controls.AddRange(new Control[] { home, settings });

            // Header: the logo draws itself as the window opens.
            var head = new Panel { Width = W, Height = 80, Margin = Padding.Empty };
            logo.Bounds = new Rectangle(0, 4, 68, 68);
            head.Controls.Add(logo);
            var title = Theme.Label("Thurea", 22f, Theme.Text, FontStyle.Bold);
            title.Bounds = new Rectangle(86, 2, 300, 36);
            head.Controls.Add(title);
            line.Bounds = new Rectangle(88, 39, W - 88, 22);
            head.Controls.Add(line);
            version.Bounds = new Rectangle(88, 61, W - 88, 18);
            head.Controls.Add(version);
            gear = NavigationButton("\u2699", "Settings", W - 36);
            gear.Click += delegate { ShowSettings(true); };
            head.Controls.Add(gear);
            home.Controls.Add(head);

            // Verse of the day.
            verseCard.Width = W;
            verseCard.BackColor = Color.FromArgb(0x1C, 0x24, 0x33);
            verseCard.Margin = new Padding(0, 10, 0, 6);
            var eyebrow = Theme.Label("VERSE OF THE DAY", 8.5f, Theme.Accent, FontStyle.Bold);
            eyebrow.Bounds = new Rectangle(18, 14, W - 36, 18);
            verseCard.Controls.Add(eyebrow);
            verseText.Location = new Point(18, 38);
            verseCard.Controls.Add(verseText);
            verseRef.TextAlign = ContentAlignment.TopRight;
            verseCard.Controls.Add(verseRef);
            home.Controls.Add(verseCard);

            var actions = new FlowLayoutPanel { Width = W, Height = 48, BackColor = Color.Transparent, Margin = new Padding(0, 10, 0, 0) };
            var phoneBtn = Theme.Button("Set up a phone", true); phoneBtn.Width = 150; phoneBtn.Click += delegate { tray.ShowPhone(); };
            var shareBtn = Theme.Button("Another device", false); shareBtn.Width = 140; shareBtn.Click += delegate { tray.ShowShare(); };
            // Parents' control center: children's requests, rules, devices (thurea.exe control-center).
            var familyBtn = Theme.Button("Family", false); familyBtn.Width = 130; familyBtn.Click += delegate { tray.OpenFamily(); };
            actions.Controls.AddRange(new Control[] { phoneBtn, shareBtn, familyBtn });
            home.Controls.Add(actions);

            var settingsHeader = new Panel { Width = W, Height = 48, Margin = Padding.Empty };
            back = NavigationButton("\u2190", "Back to Thurea", 0);
            back.Click += delegate { ShowSettings(false); };
            settingsHeader.Controls.Add(back);
            var settingsTitle = Theme.Label("Settings", 18f, Theme.Text, FontStyle.Bold);
            settingsTitle.Bounds = new Rectangle(48, 0, W - 48, 36);
            settingsHeader.Controls.Add(settingsTitle);
            settings.Controls.Add(settingsHeader);
            AddSetting("verses", "Verse of the day", "A Bible verse here each day (World English Bible)");
            AddSetting("animation", "Opening animation", "The Thurea logo draws itself when Thurea opens");

            // Protection: re-apply now instead of waiting for the 5-minute check.
            AddHeading("Protection");
            var applyBtn = Theme.Button("Re-apply protection now", false);
            applyBtn.Width = 220;
            applyBtn.Margin = new Padding(0, 8, 0, 2);
            applyBtn.Click += delegate { tray.Reapply(); };
            settings.Controls.Add(applyBtn);
            var applyHelp = Theme.Label("Thurea also checks and re-applies by itself every 5 minutes. Needs Administrator.", 9f, Theme.Muted);
            applyHelp.Size = new Size(W, 22);
            applyHelp.Margin = new Padding(0, 0, 0, 4);
            settings.Controls.Add(applyHelp);

            // About: version, copyright and licence.
            AddHeading("About");
            about.Size = new Size(W, 22);
            about.Margin = new Padding(0, 6, 0, 0);
            settings.Controls.Add(about);
            var copyright = new LinkLabel
            {
                Text = "\u00a9 2026 Luke Shaddick (ShaddickDev). Free and open source under the MIT License. Licence",
                AutoSize = false, Width = W, Height = 40, ForeColor = Theme.Muted, LinkColor = Theme.Accent, ActiveLinkColor = Theme.Accent,
                BackColor = Color.Transparent, Font = new Font("Segoe UI", 9f), Margin = new Padding(0, 4, 0, 0)
            };
            copyright.LinkArea = new LinkArea(copyright.Text.Length - "Licence".Length, "Licence".Length);
            copyright.LinkClicked += delegate { try { Process.Start("notepad.exe", "\"" + Paths.License + "\""); } catch (Exception) { } };
            settings.Controls.Add(copyright);
            var source = new LinkLabel
            {
                Text = "github.com/ShaddickDev/thurea", AutoSize = true, LinkColor = Theme.Accent, ActiveLinkColor = Theme.Accent,
                BackColor = Color.Transparent, Font = new Font("Segoe UI", 9f), Margin = new Padding(0, 2, 0, 0)
            };
            source.LinkClicked += delegate { try { Process.Start(new ProcessStartInfo("https://github.com/ShaddickDev/thurea") { UseShellExecute = true }); } catch (Exception) { } };
            settings.Controls.Add(source);

            var credits = new LinkLabel
            {
                Text = "Verses: World English Bible (public domain). Adult-site list: StevenBlack/hosts (MIT) and The Block List Project (Unlicense). "
                     + "Filters: EasyList / EasyPrivacy (GPL-3.0 or CC BY-SA 3.0). Family DNS: Cloudflare for Families. All credits",
                AutoSize = false, Width = W, Height = 64, ForeColor = Theme.Muted, LinkColor = Theme.Accent, ActiveLinkColor = Theme.Accent,
                BackColor = Color.Transparent, Font = new Font("Segoe UI", 8.5f), Margin = new Padding(0, 12, 0, 0)
            };
            credits.LinkArea = new LinkArea(credits.Text.Length - "All credits".Length, "All credits".Length);
            credits.LinkClicked += delegate { try { Process.Start("notepad.exe", "\"" + Paths.Credits + "\""); } catch (Exception) { } };
            settings.Controls.Add(credits);

            Shown += delegate { if (Prefs.Get("animation")) logo.Play(); };
        }

        static FlowLayoutPanel CreatePage()
        {
            return new FlowLayoutPanel
            {
                FlowDirection = FlowDirection.TopDown, WrapContents = false, AutoScroll = true,
                Dock = DockStyle.Fill, Padding = new Padding(24, 20, 24, 20)
            };
        }

        Button NavigationButton(string icon, string label, int x)
        {
            var button = Theme.Button(icon, false);
            button.Font = new Font("Segoe UI Symbol", 13f);
            button.Bounds = new Rectangle(x, 0, 36, 36);
            button.AccessibleName = label;
            hints.SetToolTip(button, label);
            return button;
        }

        void AddHeading(string text)
        {
            var heading = Theme.Label(text.ToUpperInvariant(), 8.5f, Theme.Accent, FontStyle.Bold);
            heading.Size = new Size(W, 18);
            heading.Margin = new Padding(0, 18, 0, 0);
            settings.Controls.Add(heading);
        }

        void AddSetting(string key, string label, string description)
        {
            var toggle = Theme.Check(label, Prefs.Get(key));
            toggle.Margin = new Padding(0, 8, 0, 2);
            toggle.AccessibleDescription = description;
            toggle.CheckedChanged += delegate { Prefs.Set(key, toggle.Checked); };
            settings.Controls.Add(toggle);
            var help = Theme.Label(description, 9f, Theme.Muted);
            help.Size = new Size(W - 22, 22);
            help.Margin = new Padding(22, 0, 0, 4);
            settings.Controls.Add(help);
        }

        void FitPage(FlowLayoutPanel page)
        {
            // Minimized, Windows reports a tiny client area: sizing now would bring the window back narrow.
            if (WindowState == FormWindowState.Minimized) return;
            int height = page.Padding.Vertical;
            foreach (Control control in page.Controls)
                if (control.Visible) height += control.Height + control.Margin.Vertical;
            int maxHeight = Screen.FromControl(this).WorkingArea.Height - (Height - ClientSize.Height);
            ClientSize = new Size(W + 48, Math.Min(height, maxHeight));
        }

        void ShowSettings(bool show)
        {
            home.Visible = !show;
            settings.Visible = show;
            if (show) { FitPage(settings); back.Focus(); }
            else { Render(Status.Read()); gear.Focus(); }
        }

        protected override bool ProcessCmdKey(ref Message msg, Keys keyData)
        {
            if (settings.Visible && keyData == Keys.Escape) { ShowSettings(false); return true; }
            return base.ProcessCmdKey(ref msg, keyData);
        }

        protected override void Dispose(bool disposing)
        {
            if (disposing) hints.Dispose();
            base.Dispose(disposing);
        }

        public void Render(Status s)
        {
            line.Text = s.Line;
            line.ForeColor = s.Ok ? Theme.Good : Theme.Warn;
            logo.Warning = s.Known && !s.Ok;
            logo.Invalidate();
            version.Text = s.Version != "" ? "Thurea " + s.Version + (s.Known ? ", checked " + s.At.ToString("t") : "") : "";
            about.Text = (s.Version != "" ? "Thurea " + s.Version : "Thurea") + "  \u00b7  family protection that blocks ads and harmful sites";

            var v = Prefs.Get("verses") ? Verses.Today() : null;
            verseCard.Visible = v != null;
            if (v != null)
            {
                verseText.Text = "\u201c" + v[1] + "\u201d";
                var size = TextRenderer.MeasureText(verseText.Text, verseText.Font, new Size(W - 36, 1000), TextFormatFlags.WordBreak);
                verseText.Size = new Size(W - 36, size.Height + 4);
                verseRef.Text = v[0] + "  \u00b7  " + v[2];
                verseRef.Bounds = new Rectangle(18, verseText.Bottom + 8, W - 36, 18);
                verseCard.Height = verseRef.Bottom + 16;
            }

            if (home.Visible) FitPage(home);
        }
    }

    // ── Phone setup by QR code (Android device-owner provisioning) ──

    static class Lan
    {
        static readonly string[] Virtual = { "vpn", "virtual", "hyper-v", "vmware", "virtualbox", "tap-", "wireguard", "tailscale", "zerotier", "loopback", "wsl", "proton", "nord", "mullvad", "bluetooth" };

        // The address other devices on the home network reach this PC at: a private IPv4 address
        // of a Wi-Fi or Ethernet adapter that's up (never a VPN's tunnel address).
        public static string Address()
        {
            try
            {
                var best = NetworkInterface.GetAllNetworkInterfaces()
                    .Where(n => n.OperationalStatus == OperationalStatus.Up
                        && (n.NetworkInterfaceType == NetworkInterfaceType.Wireless80211 || n.NetworkInterfaceType == NetworkInterfaceType.Ethernet || n.NetworkInterfaceType == NetworkInterfaceType.GigabitEthernet)
                        && !Virtual.Any(v => (n.Description + " " + n.Name).ToLowerInvariant().Contains(v)))
                    .OrderBy(n => n.NetworkInterfaceType == NetworkInterfaceType.Wireless80211 ? 0 : 1)
                    .SelectMany(n => n.GetIPProperties().UnicastAddresses.Select(a => a.Address))
                    .FirstOrDefault(a => a.AddressFamily == AddressFamily.InterNetwork && IsPrivate(a));
                return best == null ? null : best.ToString();
            }
            catch (Exception) { return null; }
        }

        static bool IsPrivate(IPAddress a)
        {
            var b = a.GetAddressBytes();
            return b[0] == 10 || (b[0] == 172 && b[1] >= 16 && b[1] <= 31) || (b[0] == 192 && b[1] == 168);
        }

        // The Wi-Fi network this PC is connected to (its name; Windows only gives the password to Administrators).
        public static string CurrentSsid()
        {
            try
            {
                var p = new ProcessStartInfo("netsh.exe", "wlan show interfaces") { UseShellExecute = false, RedirectStandardOutput = true, CreateNoWindow = true };
                using (var proc = Process.Start(p))
                {
                    string output = proc.StandardOutput.ReadToEnd();
                    proc.WaitForExit(5000);
                    foreach (var l in output.Split('\n'))
                    {
                        var t = l.Trim();
                        if (t.StartsWith("SSID") && !t.StartsWith("SSID name") && t.Contains(":")) return t.Substring(t.IndexOf(':') + 1).Trim();
                    }
                }
            }
            catch (Exception) { }
            return "";
        }

        public static string Token()
        {
            byte[] rnd = new byte[16];
            using (var r = new RNGCryptoServiceProvider()) r.GetBytes(rnd);
            return BitConverter.ToString(rnd).Replace("-", "").ToLowerInvariant();
        }
    }

    class PhoneForm : ThureaForm
    {
        const int Port = 47824;
        HttpListener listener;
        string token, url, apkSum;
        readonly PictureBox pic = new PictureBox { SizeMode = PictureBoxSizeMode.Zoom, BackColor = Theme.Bg };
        readonly Label progress = Theme.Label("", 9.5f, Theme.Muted);
        readonly TextBox ssid = new TextBox(), pass = new TextBox { UseSystemPasswordChar = true };
        readonly CheckBox withWifi;
        // "Keep my phone": a phone in use, nothing erased. thurea.exe adopt-phone pairs with it by
        // Android's Wireless debugging QR (or USB) and does the setup; this window shows its progress.
        readonly List<Control> resetControls = new List<Control>();
        readonly Panel keepPanel = new Panel { BackColor = Color.Transparent };
        readonly PictureBox keepPic = new PictureBox { SizeMode = PictureBoxSizeMode.Zoom, BackColor = Theme.Bg };
        readonly Label keepProgress = Theme.Label("", 10f, Theme.Text, FontStyle.Bold);
        readonly Button keepTab = Theme.Button("Keep my phone", false), resetTab = Theme.Button("Reset phone", false);
        Process adopt;
        List<string> pendingRows = new List<string>();

        public PhoneForm()
        {
            Text = "Set up a phone with Thurea";
            ClientSize = new Size(520, 800);

            var logo = new LogoView { Bounds = new Rectangle(236, 14, 48, 48) };
            Controls.Add(logo);
            var title = Theme.Label("SET UP A PHONE WITH THUREA", 9f, Theme.Accent, FontStyle.Bold);
            title.TextAlign = ContentAlignment.MiddleCenter; title.Bounds = new Rectangle(0, 66, 520, 22);
            Controls.Add(title);
            pic.Bounds = new Rectangle(90, 96, 340, 340);
            Controls.Add(pic);
            var steps = Theme.Label("On a new or factory-reset Android phone, tap the same spot on the first Welcome screen 6 times, then scan this code. "
                + "The phone downloads Thurea from this PC (same network), checks it, and Thurea becomes its device owner before the home screen appears.", 10f, Theme.Text);
            steps.TextAlign = ContentAlignment.TopCenter; steps.Bounds = new Rectangle(24, 446, 472, 84);
            Controls.Add(steps);
            progress.TextAlign = ContentAlignment.TopCenter; progress.Bounds = new Rectangle(24, 532, 472, 40);
            Controls.Add(progress);

            withWifi = Theme.Check("Put this Wi-Fi in the code, so the phone joins it by itself", true);
            withWifi.Location = new Point(40, 580);
            Controls.Add(withWifi);
            var l1 = Theme.Label("Wi-Fi name", 9.5f, Theme.Muted); l1.Bounds = new Rectangle(40, 614, 120, 24); Controls.Add(l1);
            ssid.Bounds = new Rectangle(170, 612, 300, 26); ssid.Text = Lan.CurrentSsid(); Controls.Add(ssid);
            var l2 = Theme.Label("Wi-Fi password", 9.5f, Theme.Muted); l2.Bounds = new Rectangle(40, 648, 120, 24); Controls.Add(l2);
            pass.Bounds = new Rectangle(170, 646, 300, 26); Controls.Add(pass);
            var update = Theme.Button("Update the code", false); update.Bounds = new Rectangle(170, 684, 180, 36);
            update.Click += delegate { MakeCode(); };
            Controls.Add(update);
            withWifi.CheckedChanged += delegate { ssid.Enabled = pass.Enabled = withWifi.Checked; MakeCode(); };
            var note = Theme.Label("The password only goes into the code on screen; it isn't saved. The download stops when this window closes.", 8.5f, Theme.Muted);
            note.TextAlign = ContentAlignment.TopCenter; note.Bounds = new Rectangle(24, 732, 472, 40);
            Controls.Add(note);

            // Mode tabs: everything above belongs to "Reset phone" and moves down under them.
            foreach (Control c in Controls) if (c != logo && c != title) { c.Top += 50; resetControls.Add(c); }
            ClientSize = new Size(520, 850);
            keepTab.Bounds = new Rectangle(40, 96, 214, 36);
            resetTab.Bounds = new Rectangle(266, 96, 214, 36);
            resetTab.Click += delegate { SetMode(false); };
            keepTab.Click += delegate { SetMode(true); };
            Controls.Add(keepTab); Controls.Add(resetTab);

            keepPanel.Bounds = new Rectangle(0, 146, 520, 700);
            keepPic.Bounds = new Rectangle(115, 0, 290, 290);
            keepPanel.Controls.Add(keepPic);
            keepProgress.TextAlign = ContentAlignment.TopCenter; keepProgress.Bounds = new Rectangle(24, 300, 472, 48);
            keepPanel.Controls.Add(keepProgress);
            var keepSteps = Theme.Label(
                "1.  On the phone: Settings > About phone > tap Build number 7 times. (Or plug it in by USB with USB debugging on, and skip to 3.)\n\n"
              + "2.  Settings > System > Developer options > Wireless debugging: on > Pair device with QR code.\n\n"
              + "3.  Scan this code. Thurea installs over Wi-Fi, signs your accounts out for a moment (Android needs that once), takes charge of the phone, then signs you back in.", 10f, Theme.Text);
            keepSteps.Bounds = new Rectangle(40, 354, 440, 190);
            keepPanel.Controls.Add(keepSteps);
            var keepNote = Theme.Label("Nothing on the phone is erased: apps, photos and settings stay. You enter passwords on the phone; Thurea never types or sees them. "
                + "Needs Android's platform tools (adb) on this PC.", 8.5f, Theme.Muted);
            keepNote.TextAlign = ContentAlignment.TopCenter; keepNote.Bounds = new Rectangle(24, 558, 472, 60);
            keepPanel.Controls.Add(keepNote);
            keepPanel.Visible = false;
            Controls.Add(keepPanel);

            Shown += delegate { if (Prefs.Get("animation")) logo.Play(); };
            Start();
        }

        public void SetMode(bool keep)
        {
            foreach (var c in resetControls) c.Visible = !keep;
            keepPanel.Visible = keep;
            resetTab.BackColor = keep ? Theme.Card : Theme.Accent; resetTab.ForeColor = keep ? Theme.Text : Theme.Bg;
            keepTab.BackColor = keep ? Theme.Accent : Theme.Card; keepTab.ForeColor = keep ? Theme.Bg : Theme.Text;
            if (keep && adopt == null) StartAdopt();
        }

        void StartAdopt()
        {
            string exe = Path.Combine(Paths.Home, "thurea.exe");
            keepProgress.Text = "Preparing the pairing code...";
            pendingRows = new List<string>();
            try
            {
                var psi = new ProcessStartInfo(exe, "adopt-phone --tray --apk \"" + Paths.Apk + "\"")
                { UseShellExecute = false, RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true, StandardOutputEncoding = Encoding.UTF8 };
                adopt = Process.Start(psi);
            }
            catch (Exception e) { keepProgress.Text = "Couldn't start Thurea's phone setup: " + e.Message; return; }
            var p = adopt;
            new Thread(() =>
            {
                string l;
                while ((l = p.StandardOutput.ReadLine()) != null) { var line = l; try { BeginInvoke((MethodInvoker)delegate { AdoptLine(line); }); } catch (Exception) { return; } }
            }) { IsBackground = true }.Start();
            new Thread(() => { try { p.StandardError.ReadToEnd(); } catch (Exception) { } }) { IsBackground = true }.Start();
        }

        static readonly Dictionary<string, string> AdoptText = new Dictionary<string, string> {
            { "found", "Found the phone. Pairing..." }, { "paired", "Paired. Connecting..." }, { "connected", "Connected to the phone" },
            { "phone", "Setting up {0}" }, { "installing", "Installing Thurea..." }, { "installed", "Thurea is installed" },
            { "signout", "Signing out for a moment: {0}. Confirm on the phone, and enter your PIN there if it asks." },
            { "signout-wait", "Signing out for a moment: {0}. Confirm on the phone, and enter your PIN there if it asks." },
            { "owner", "Thurea is in charge of the phone. Signing your accounts back in..." },
            { "owner-failed", "Thurea couldn't take charge ({0}). Signing your accounts back in." },
            { "signin", "Signing back in to {0}: enter your password or use your passkey on the phone." },
            { "signin-samsung", "Add account is open on the phone: sign back in to your Samsung account ({0})." },
            { "signedin", "{0} is back." }, { "signin-later", "{0} isn't back yet: add it later in Settings > Accounts." },
            { "apps", "Open these apps once to sign back in: {0}" },
            { "done", "All done. Thurea is in charge, your accounts are back, and Wireless debugging is off again." } };

        void AdoptLine(string l)
        {
            var f = l.Split('\t');
            if (f[0] == "m" && f.Length > 1) { pendingRows.Add(f[1]); return; }
            if (f[0] == "ready")
            {
                int n = pendingRows.Count;
                if (n > 0 && pendingRows[0].Length == n)
                {
                    var old = keepPic.Image; keepPic.Image = QrArt.Draw(pendingRows, 12, Theme.Bg, true); if (old != null) old.Dispose();
                }
                keepProgress.Text = "Waiting for the phone to scan (same Wi-Fi as this PC)";
                return;
            }
            if (f[0] == "event" && f.Length > 1)
            {
                string t;
                if (f[1] == "connected") keepPic.Visible = false;
                if (f[1] == "done" && f.Length > 2 && f[2] == "updated") keepProgress.Text = "Thurea was already in charge of this phone. It's updated.";
                else if (AdoptText.TryGetValue(f[1], out t)) keepProgress.Text = string.Format(t, f.Length > 2 ? f[2] : "");
                return;
            }
            if (f[0] == "error") keepProgress.Text = string.Join(" ", f, 1, f.Length - 1);
        }

        void Start()
        {
            if (!File.Exists(Paths.Apk)) { progress.Text = "Thurea for Android isn't on this PC (reinstall Thurea)."; return; }
            string ip = Lan.Address();
            if (ip == null) { progress.Text = "This PC isn't on a home network. Connect it to the same Wi-Fi as the phone, then open this again."; return; }
            using (var sha = SHA256.Create()) using (var f = File.OpenRead(Paths.Apk))
                apkSum = Convert.ToBase64String(sha.ComputeHash(f)).TrimEnd('=').Replace('+', '-').Replace('/', '_');
            token = Lan.Token();
            url = "http://" + ip + ":" + Port + "/" + token + "/Thurea.apk";
            try
            {
                listener = new HttpListener();
                listener.Prefixes.Add("http://+:" + Port + "/" + token + "/");
                listener.Start();
                new Thread(Serve) { IsBackground = true }.Start();
            }
            catch (Exception e) { progress.Text = "Couldn't open the download on this PC: " + e.Message; return; }
            MakeCode();
        }

        static string Json(string s)
        {
            var sb = new StringBuilder("\"");
            foreach (char c in s)
            {
                if (c == '"' || c == '\\') sb.Append('\\').Append(c);
                else if (c < 0x20) sb.Append("\\u").Append(((int)c).ToString("x4"));
                else sb.Append(c);
            }
            return sb.Append('"').ToString();
        }

        // Android's provisioning QR: a JSON object of android.app.extra.PROVISIONING_* keys (the
        // same code `thurea setup-phone` makes on Linux).
        void MakeCode()
        {
            if (url == null) return;
            const string k = "\"android.app.extra.PROVISIONING_";
            var j = new StringBuilder("{");
            j.Append(k).Append("DEVICE_ADMIN_COMPONENT_NAME\":\"com.thurea.guard/.AdminReceiver\",");
            j.Append(k).Append("DEVICE_ADMIN_PACKAGE_DOWNLOAD_LOCATION\":").Append(Json(url)).Append(',');
            j.Append(k).Append("DEVICE_ADMIN_PACKAGE_CHECKSUM\":").Append(Json(apkSum)).Append(',');
            j.Append(k).Append("LEAVE_ALL_SYSTEM_APPS_ENABLED\":true,");
            j.Append(k).Append("SKIP_EDUCATION_SCREENS\":true,");
            if (withWifi.Checked && ssid.Text.Trim() != "")
            {
                j.Append(k).Append("WIFI_SSID\":").Append(Json(ssid.Text.Trim())).Append(',');
                j.Append(k).Append("WIFI_SECURITY_TYPE\":").Append(pass.Text == "" ? "\"NONE\"" : "\"WPA\"").Append(',');
                if (pass.Text != "") j.Append(k).Append("WIFI_PASSWORD\":").Append(Json(pass.Text)).Append(',');
            }
            j.Append(k).Append("ADMIN_EXTRAS_BUNDLE\":{}}");
            try
            {
                var old = pic.Image;
                pic.Image = QrArt.Draw(QrCode.Encode(Encoding.UTF8.GetBytes(j.ToString())), 6, Theme.Bg, false);
                if (old != null) old.Dispose();
                progress.Text = "Waiting for the phone... (it must be on the same network as this PC)";
            }
            catch (Exception e) { progress.Text = "Couldn't make the code: " + e.Message; }
        }

        void Say(string s) { try { BeginInvoke((MethodInvoker)delegate { progress.Text = s; }); } catch (Exception) { } }

        void Serve()
        {
            while (listener != null && listener.IsListening)
            {
                HttpListenerContext c;
                try { c = listener.GetContext(); } catch (Exception) { return; }
                string who = c.Request.RemoteEndPoint.Address.ToString();
                try
                {
                    if (c.Request.Url.AbsolutePath == "/" + token + "/Thurea.apk" && (c.Request.HttpMethod == "GET" || c.Request.HttpMethod == "HEAD"))
                    {
                        Say(who + " is downloading Thurea...");
                        c.Response.ContentType = "application/vnd.android.package-archive";
                        c.Response.AddHeader("Cache-Control", "no-store");
                        using (var s = File.OpenRead(Paths.Apk))
                        {
                            c.Response.ContentLength64 = s.Length;
                            if (c.Request.HttpMethod == "GET") s.CopyTo(c.Response.OutputStream);
                        }
                        if (c.Request.HttpMethod == "GET") Say("The phone has Thurea and is setting itself up. You can close this window once it shows Thurea.");
                    }
                    else c.Response.StatusCode = 404;
                }
                catch (Exception e) { Say("The download to " + who + " broke off (" + e.Message + "); the phone will retry."); }
                finally { try { c.Response.Close(); } catch (Exception) { } }
            }
        }

        protected override void OnFormClosed(FormClosedEventArgs e)
        {
            try { if (listener != null) listener.Close(); } catch (Exception) { }
            listener = null;
            try { if (adopt != null && !adopt.HasExited) adopt.Kill(); } catch (Exception) { }
            base.OnFormClosed(e);
        }
    }

    // ── "Get Thurea on another device": a page on this PC to download Thurea for Android / Windows ──

    class ShareForm : ThureaForm
    {
        const int Port = 47824;
        HttpListener listener;
        string token;
        readonly Label status = Theme.Label("", 9.5f, Theme.Muted);

        public ShareForm()
        {
            Text = "Get Thurea on another device";
            MinimizeBox = false;
            ClientSize = new Size(440, 580);

            string ip = Lan.Address();
            if (ip == null) { AddText("This PC isn't on a home network. Connect it to the same Wi-Fi as the other device, then try again.", 20); return; }
            token = Lan.Token();
            string url = "http://" + ip + ":" + Port + "/" + token + "/";
            try
            {
                listener = new HttpListener();
                listener.Prefixes.Add("http://+:" + Port + "/" + token + "/");
                listener.Start();
                new Thread(Serve) { IsBackground = true }.Start();
            }
            catch (Exception e) { AddText("Couldn't open the page on this PC: " + e.Message, 20); return; }

            var qr = QrCode.Encode(Encoding.UTF8.GetBytes(url));
            var pic = new PictureBox { Image = qr.ToBitmap(8), SizeMode = PictureBoxSizeMode.Zoom, Bounds = new Rectangle(40, 16, 360, 360), BackColor = Color.White };
            Controls.Add(pic);
            AddText("For a phone that's already set up, or another PC: scan with its camera (same Wi-Fi as this PC). "
                + "To set up a new or reset phone fully, use \"Set up a phone\" instead.", 384);
            var link = new LinkLabel { Text = url, AutoSize = false, TextAlign = ContentAlignment.MiddleCenter, Bounds = new Rectangle(10, 466, 420, 24), LinkColor = Theme.Accent, ActiveLinkColor = Theme.Accent };
            link.LinkClicked += delegate { Clipboard.SetText(url); status.Text = "Address copied."; };
            Controls.Add(link);
            status.Bounds = new Rectangle(10, 494, 420, 44); status.TextAlign = ContentAlignment.TopCenter;
            status.Text = "The page closes when you close this window.";
            Controls.Add(status);
        }

        void AddText(string t, int y)
        {
            var l = Theme.Label(t, 10f, Theme.Text);
            l.TextAlign = ContentAlignment.TopCenter; l.Bounds = new Rectangle(16, y, 408, 76);
            Controls.Add(l);
        }

        protected override void OnFormClosed(FormClosedEventArgs e)
        {
            try { if (listener != null) listener.Close(); } catch (Exception) { }
            listener = null;
            base.OnFormClosed(e);
        }

        void Say(string s) { try { BeginInvoke((MethodInvoker)delegate { status.Text = s; }); } catch (Exception) { } }

        void Serve()
        {
            while (listener != null && listener.IsListening)
            {
                HttpListenerContext c;
                try { c = listener.GetContext(); } catch (Exception) { return; }
                try
                {
                    string path = c.Request.Url.AbsolutePath;
                    string rest = path.Length > token.Length + 2 ? path.Substring(token.Length + 2) : "";
                    string who = c.Request.RemoteEndPoint.Address.ToString();
                    if (rest == "Thurea.apk" && File.Exists(Paths.Apk)) { Say(who + " is downloading Thurea for Android"); SendFile(c, Paths.Apk, "application/vnd.android.package-archive"); }
                    else if (rest == "thurea-windows.zip" && File.Exists(Paths.WinZip)) { Say(who + " is downloading Thurea for Windows"); SendFile(c, Paths.WinZip, "application/zip"); }
                    else if (rest == "") { SendPage(c); Say(who + " opened the page"); }
                    else { c.Response.StatusCode = 404; }
                }
                catch (Exception) { }
                finally { try { c.Response.Close(); } catch (Exception) { } }
            }
        }

        static void SendFile(HttpListenerContext c, string f, string type)
        {
            c.Response.ContentType = type;
            c.Response.AddHeader("Content-Disposition", "attachment; filename=\"" + Path.GetFileName(f) + "\"");
            using (var s = File.OpenRead(f)) { c.Response.ContentLength64 = s.Length; s.CopyTo(c.Response.OutputStream); }
        }

        static void SendPage(HttpListenerContext c)
        {
            string html = "<!doctype html><html><head><meta charset=utf-8><meta name=viewport content='width=device-width,initial-scale=1'><title>Thurea</title>"
                + "<style>body{font-family:system-ui,sans-serif;margin:0;padding:24px;background:#111318;color:#eceef2}h1{font-size:24px}"
                + "a.b{display:block;margin:14px 0;padding:18px;border-radius:12px;background:#8cc8ff;color:#111318;text-decoration:none;font-size:18px;text-align:center;font-weight:600}"
                + "p{color:#c9ced6;line-height:1.45}.n{font-size:14px;color:#9aa0aa}</style></head><body><h1>&#128737; Thurea</h1>"
                + "<p>Family protection that blocks ads and harmful sites. Choose this device:</p>"
                + (File.Exists(Paths.Apk) ? "<a class=b href='Thurea.apk'>Thurea for Android</a><p class=n>Open the downloaded file and allow installing from your browser if Android asks. "
                    + "For full protection on a new or reset phone, use <i>Set up a phone</i> in Thurea on the PC instead.</p>" : "")
                + (File.Exists(Paths.WinZip) ? "<a class=b href='thurea-windows.zip'>Thurea for Windows</a><p class=n>Unzip it on the PC, then right-click <b>Install Thurea.cmd</b> and choose <b>Run as administrator</b>.</p>" : "")
                + "</body></html>";
            byte[] b = Encoding.UTF8.GetBytes(html);
            c.Response.ContentType = "text/html; charset=utf-8";
            c.Response.ContentLength64 = b.Length;
            c.Response.OutputStream.Write(b, 0, b.Length);
        }
    }
}
