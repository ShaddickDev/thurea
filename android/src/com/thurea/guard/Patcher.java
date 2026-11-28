package com.thurea.guard;

import android.content.Context;
import android.content.SharedPreferences;
import android.content.pm.PackageInfo;
import android.content.pm.PackageManager;
import android.os.Build;
import com.reandroid.apk.ApkBundle;
import com.reandroid.apk.ApkModule;
import com.reandroid.arsc.chunk.xml.AndroidManifestBlock;
import com.reandroid.arsc.chunk.xml.ResXmlAttribute;
import com.reandroid.arsc.chunk.xml.ResXmlElement;
import dalvik.system.DexClassLoader;
import java.io.ByteArrayOutputStream;
import java.io.File;
import java.io.FileOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.lang.reflect.InvocationHandler;
import java.lang.reflect.Method;
import java.lang.reflect.Proxy;
import java.net.HttpURLConnection;
import java.net.URL;
import java.security.MessageDigest;
import java.security.SecureRandom;
import java.security.cert.X509Certificate;
import java.util.ArrayList;
import java.util.Collection;
import java.util.HashSet;
import java.util.List;
import java.util.Set;
import java.util.TreeSet;
import java.util.zip.ZipEntry;
import java.util.zip.ZipInputStream;
import org.json.JSONArray;
import org.json.JSONObject;

/**
 * Patches Twitch on the phone with ReVanced's patches; no computer, no ReVanced Manager install.
 *
 * Every download is checked before use (the checks themselves are in Rust, twitch.rs):
 * - the patches (api.revanced.app) carry ReVanced's OpenPGP signature, from the key Thurea pins;
 * - the patcher comes from ReVanced Manager's APK, which carries the same signature and Manager's
 *   own signing certificate. Thurea loads the patcher's classes from it and never installs it;
 * - every APK of the Twitch bundle is signed with Twitch's own certificate (checked with apksig,
 *   which verifies the whole file).
 *
 * The bundle's split APKs are merged into one (ARSCLib), patched (Manager's patcher, with the aapt2
 * Thurea carries in its native library folder), signed with a key that stays on this phone, and
 * handed to Pins, which installs and pins it like any other pinned app.
 *
 * Runs on a background thread; the steps can take a few minutes and a few hundred MB of memory.
 */
final class Patcher {
    static final String API = "https://api.revanced.app/v5/";
    static final String TWITCH = "tv.twitch.android.app";
    private static final String PREFS = "patcher";
    private static final long MB = 1 << 20;

    static SharedPreferences prefs(Context c) { return c.getSharedPreferences(PREFS, Context.MODE_PRIVATE); }

    static File work(Context c) { return new File(c.getFilesDir(), "patcher"); }

    static boolean running(Context c) { return prefs(c).getBoolean("running", false); }

    /** The last status line, for the Pinned apps screen. */
    static String status(Context c) { return prefs(c).getString("status", ""); }

    private static void status(Context c, String s) { prefs(c).edit().putString("status", s).putLong("at", System.currentTimeMillis()).apply(); }

    /** Patches the Twitch bundle (.apkm, or a folder of split APKs) at `bundle` and pins it. */
    static void patchTwitch(Context c, File bundle) {
        Context app = c.getApplicationContext();
        if (running(app)) { status(app, "Already patching Twitch…"); return; }
        prefs(app).edit().putBoolean("running", true).apply();
        new Thread(() -> {
            try { run(app, bundle); }
            catch (Throwable t) {
                String why = t.getMessage() == null ? t.toString() : t.getMessage();
                status(app, "! Couldn't patch Twitch: " + why);
                Pins.notify(app, 4, "Couldn't patch Twitch", why + "\n\nYour current Twitch is untouched.");
            } finally {
                prefs(app).edit().putBoolean("running", false).apply();
            }
        }, "thurea-patcher").start();
    }

    private static void run(Context c, File bundle) throws Exception {
        if (!Policy.isOwner(c)) throw new Exception("Thurea isn't the device owner");
        if (Pins.busy(c)) throw new Exception("pinned apps are being installed; try again in a minute");
        File w = work(c); w.mkdirs();
        String keys = "";
        try { keys = text(API + "patches/keys", MB); } catch (Exception ignored) {}   // the built-in key then

        status(c, "Checking ReVanced's patches…");
        JSONObject rel = new JSONObject(Native.patchesRelease(text(API + "patches", MB)));
        if (!rel.has("tag")) throw new Exception("ReVanced's patches release couldn't be read");
        String tag = rel.getString("tag");
        File rvp = new File(w, "patches-" + tag + ".rvp"), rvpSig = new File(w, "patches-" + tag + ".rvp.asc");
        fetchSigned(c, rel.getString("rvp"), rvp, rel.getString("asc"), rvpSig, keys, 64 * MB);

        status(c, "Checking ReVanced's patcher…");
        JSONObject mgr = new JSONObject(Native.managerRelease(text(API + "manager", MB)));
        if (!mgr.has("tag")) throw new Exception("ReVanced Manager's release couldn't be read");
        File mApk = new File(w, "manager-" + mgr.getString("tag") + ".apk"), mSig = new File(mApk.getPath() + ".asc");
        fetchSigned(c, mgr.getString("apk"), mApk, mgr.getString("asc"), mSig, keys, 200 * MB);
        if (!Native.genuineManager(certs(c, mApk))) throw new Exception("ReVanced Manager's APK isn't signed with Manager's certificate");
        prune(w, rvp, rvpSig, mApk, mSig);
        Rv rv = new Rv(c, mApk);

        status(c, "Checking the Twitch download…");
        File parts = new File(w, "parts"); wipe(parts); parts.mkdirs();
        List<File> apks = unpack(bundle, parts);
        for (File f : apks)
            if (!Native.genuineTwitch(rv.verifiedCerts(c, f))) throw new Exception(f.getName() + " isn't signed by Twitch — not used");

        status(c, "Merging Twitch's split APKs…");
        File merged = new File(w, "twitch-merged.apk");
        merge(parts, keep(apks), merged);
        wipe(parts);
        PackageInfo mi = c.getPackageManager().getPackageArchiveInfo(merged.getPath(), 0);
        if (mi == null || !TWITCH.equals(mi.packageName)) throw new Exception("the bundle isn't Twitch");
        String version = mi.versionName;

        status(c, "Loading ReVanced's patches " + tag + "…");
        Set<Object> chosen = rv.twitchPatches(rvp);
        JSONArray compatible = rv.compatible(chosen);
        if (compatible.length() > 0) {        // [] : every chosen patch works on any version
            boolean supported = false;
            for (int i = 0; i < compatible.length(); i++) supported |= compatible.getString(i).equals(version);
            JSONObject plan = new JSONObject(Native.twitchPlan("{}", tag, compatible.toString()));
            if (!supported) throw new Exception("ReVanced's patches " + tag + " support Twitch " + plan.optString("twitch", compatible.toString()) + ", not " + version);
        }

        status(c, "Patching Twitch " + version + " (this takes a few minutes)…");
        File patched = new File(w, "twitch-patched-unsigned.apk");
        List<String> failed = rv.patch(c, merged, chosen, patched);
        merged.delete();
        if (!failed.isEmpty()) { patched.delete(); throw new Exception("patches failed: " + String.join("; ", failed)); }

        status(c, "Signing Twitch…");
        File signed = new File(Pins.dir(c), "twitch-patched.apk");
        Pins.dir(c).mkdirs();
        rv.sign(c, patched, signed);
        patched.delete();

        status(c, "Installing patched Twitch " + version + "…");
        install(c, signed, version, tag);
    }

    // ── Downloads ──

    /** Downloads file and its signature (unless already here and still valid) and checks the signature. */
    private static void fetchSigned(Context c, String url, File f, String sigUrl, File sig, String keys, long limit) throws Exception {
        if (f.isFile() && sig.isFile() && Native.revancedSigned(f.getPath(), sig.getPath(), keys).isEmpty()) return;
        download(sigUrl, sig, MB);
        download(url, f, limit);
        String why = Native.revancedSigned(f.getPath(), sig.getPath(), keys);
        if (!why.isEmpty()) { f.delete(); sig.delete(); throw new Exception(f.getName() + ": " + why); }
    }

    /** Removes older patches and Manager downloads. */
    private static void prune(File w, File... keep) {
        Set<String> k = new HashSet<>();
        for (File f : keep) k.add(f.getName());
        File[] all = w.listFiles();
        if (all != null) for (File f : all)
            if (f.isFile() && !k.contains(f.getName()) && (f.getName().startsWith("patches-") || f.getName().startsWith("manager-"))) f.delete();
    }

    static String text(String url, long limit) throws Exception {
        ByteArrayOutputStream bo = new ByteArrayOutputStream();
        fetch(url, bo, limit);
        return bo.toString("UTF-8");
    }

    static void download(String url, File f, long limit) throws Exception {
        File part = new File(f.getPath() + ".part");
        try (OutputStream o = new FileOutputStream(part)) { fetch(url, o, limit); }
        catch (Exception e) { part.delete(); throw e; }
        if (!part.renameTo(f)) { part.delete(); throw new Exception("couldn't save " + f.getName()); }
    }

    private static void fetch(String url, OutputStream out, long limit) throws Exception {
        for (int hop = 0; hop < 6; hop++) {   // GitHub and APKMirror redirect downloads to their CDNs
            if (!url.startsWith("https://")) throw new Exception("not HTTPS: " + url);
            HttpURLConnection h = (HttpURLConnection) new URL(url).openConnection();
            h.setConnectTimeout(20000); h.setReadTimeout(60000);
            h.setInstanceFollowRedirects(false);
            h.setRequestProperty("User-Agent", "Thurea");
            int code = h.getResponseCode();
            if (code / 100 == 3) {
                URL next = new URL(new URL(url), h.getHeaderField("Location"));
                h.disconnect();
                url = next.toString(); continue;
            }
            if (code != 200) throw new Exception(url + ": HTTP " + code);
            try (InputStream in = h.getInputStream()) {
                byte[] buf = new byte[1 << 16]; int n; long total = 0;
                while ((n = in.read(buf)) > 0) { total += n; if (total > limit) throw new Exception(url + ": too large"); out.write(buf, 0, n); }
                return;
            }
        }
        throw new Exception(url + ": too many redirects");
    }

    // ── The Twitch bundle ──

    /** The APKs of a bundle: an .apkm/.apks zip of APKs, a folder of them, or a single APK. */
    static List<File> unpack(File bundle, File into) throws Exception {
        List<File> out = new ArrayList<>();
        if (bundle.isDirectory()) {
            File[] all = bundle.listFiles();
            if (all != null) for (File f : all) if (f.getName().endsWith(".apk")) { File t = new File(into, f.getName()); copy(f, t); out.add(t); }
        } else if (isApk(bundle)) {
            File t = new File(into, "base.apk"); copy(bundle, t); out.add(t);
        } else {
            long total = 0;
            try (ZipInputStream z = new ZipInputStream(new java.io.FileInputStream(bundle))) {
                for (ZipEntry e; (e = z.getNextEntry()) != null; ) {
                    String name = e.getName();
                    if (e.isDirectory() || !name.endsWith(".apk")) continue;
                    if (name.contains("/") || name.contains("\\") || name.startsWith(".")) throw new Exception("unexpected file in the bundle: " + name);
                    File t = new File(into, name);
                    try (OutputStream o = new FileOutputStream(t)) {
                        byte[] buf = new byte[1 << 16]; int n;
                        while ((n = z.read(buf)) > 0) { total += n; if (total > 1024 * MB) throw new Exception("the bundle is too large"); o.write(buf, 0, n); }
                    }
                    out.add(t);
                }
            }
        }
        boolean base = false;
        for (File f : out) base |= f.getName().equals("base.apk");
        if (!base) throw new Exception("the bundle has no base.apk");
        return out;
    }

    /** A plain APK has AndroidManifest.xml at its root; a bundle has APKs instead. */
    private static boolean isApk(File f) {
        try (java.util.zip.ZipFile z = new java.util.zip.ZipFile(f)) { return z.getEntry("AndroidManifest.xml") != null; }
        catch (Exception e) { return false; }
    }

    /**
     * The splits this phone needs: base, this phone's processor (ABI) and screen density, and every
     * language. Leaving out the other ABIs and densities keeps the patched app (and the patching)
     * a fraction of the size.
     */
    static List<File> keep(List<File> apks) {
        Set<String> abis = new HashSet<>();
        for (String a : new String[]{ "arm64_v8a", "armeabi_v7a", "armeabi", "x86", "x86_64", "mips", "mips64" }) abis.add(a);
        Set<String> dpis = new HashSet<>();
        for (String d : new String[]{ "ldpi", "mdpi", "tvdpi", "hdpi", "xhdpi", "xxhdpi", "xxxhdpi", "nodpi" }) dpis.add(d);
        String abi = Build.SUPPORTED_ABIS[0].replace('-', '_');
        String dpi = density(android.content.res.Resources.getSystem().getDisplayMetrics().densityDpi);
        boolean haveAbi = false, haveDpi = false;
        for (File f : apks) { String s = split(f); haveAbi |= s.equals(abi); haveDpi |= s.equals(dpi); }
        List<File> out = new ArrayList<>();
        for (File f : apks) {
            String s = split(f);
            if (abis.contains(s) && haveAbi && !s.equals(abi)) continue;
            if (dpis.contains(s) && haveDpi && !s.equals(dpi)) continue;
            out.add(f);
        }
        return out;
    }

    /** "split_config.arm64_v8a.apk" → "arm64_v8a"; "base.apk" → "". */
    private static String split(File f) {
        String n = f.getName().replace(".apk", "");
        int i = n.lastIndexOf("config.");
        return i < 0 ? "" : n.substring(i + 7);
    }

    private static String density(int dpi) {
        if (dpi <= 120) return "ldpi";
        if (dpi <= 160) return "mdpi";
        if (dpi <= 213) return "tvdpi";
        if (dpi <= 240) return "hdpi";
        if (dpi <= 320) return "xhdpi";
        if (dpi <= 480) return "xxhdpi";
        return "xxxhdpi";
    }

    /** Merges the split APKs `apks` (all in `dir`) into one installable APK. */
    static void merge(File dir, List<File> apks, File out) throws Exception {
        File only = new File(dir, "keep"); only.mkdirs();
        for (File f : apks) if (!f.renameTo(new File(only, f.getName()))) throw new Exception("couldn't stage " + f.getName());
        try (ApkBundle b = new ApkBundle()) {
            b.loadApkDirectory(only);
            ApkModule m = b.mergeModules();
            m.removeDir("META-INF");                 // the splits' signatures: the patched app gets its own
            AndroidManifestBlock man = m.getAndroidManifest();
            ResXmlElement root = man.getManifestElement(), application = man.getApplicationElement();
            for (ResXmlElement e : new ResXmlElement[]{ root, application }) {
                if (e == null) continue;
                e.removeAttributesWithId(0x01010591);       // android:isSplitRequired
                e.removeAttributesWithName("isSplitRequired");
                e.removeAttributesWithName("requiredSplitTypes");
                e.removeAttributesWithName("splitTypes");
            }
            if (application != null) {
                // Play's split bookkeeping, which would make Android refuse the merged APK.
                application.removeElementsIf(x -> {
                    ResXmlElement el = (ResXmlElement) x;
                    if (!"meta-data".equals(el.getName())) return false;
                    ResXmlAttribute n = el.searchAttributeByResourceId(0x01010003);   // android:name
                    String v = n == null ? "" : n.getValueString();
                    return v != null && (v.startsWith("com.android.vending.splits") || v.startsWith("com.android.stamp.") || v.equals("com.android.vending.derived.apk.id"));
                });
            }
            m.setExtractNativeLibs(true);
            m.refreshManifest();
            m.writeApk(out);
            m.close();
        }
    }

    // ── Installing ──

    /** Hands the signed build to Pins, which installs it, keeps a backup and pins it. */
    private static void install(Context c, File apk, String version, String tag) throws Exception {
        String have = Pins.installedVersion(c, TWITCH);
        boolean fresh = have != null && (!sameSigner(c, apk) || Native.newerVersion(have, version));
        SharedPreferences.Editor e = Pins.prefs(c).edit();
        for (String[] kv : new String[][]{ { "package", TWITCH }, { "official", "" }, { "version", version }, { "patches", "ReVanced " + tag } })
            e.putString("new.twitch." + kv[0], kv[1]);
        e.putString("new.twitch.src", apk.getPath()).putBoolean("new.twitch.fresh", fresh)
         .putString("twitch.source", "thurea")
         .putString("queue", "twitch").putString("log", "").commit();
        prefs(c).edit().putString("twitch.patches", tag).putString("twitch.version", version).apply();
        Pins.next(c);
        status(c, "✓ Patched Twitch " + version + " with ReVanced's patches " + tag + (fresh ? " (sign in to Twitch again once)" : ""));
    }

    /** Is the installed Twitch signed with the same key as `apk` (so it can update in place)? */
    private static boolean sameSigner(Context c, File apk) {
        try {
            PackageManager pm = c.getPackageManager();
            PackageInfo a = pm.getPackageArchiveInfo(apk.getPath(), PackageManager.GET_SIGNING_CERTIFICATES);
            PackageInfo i = pm.getPackageInfo(TWITCH, PackageManager.GET_SIGNING_CERTIFICATES);
            return a != null && a.signingInfo != null && i.signingInfo != null
                && a.signingInfo.getApkContentsSigners()[0].equals(i.signingInfo.getApkContentsSigners()[0]);
        } catch (Exception e) { return false; }
    }

    /** SHA-256s of a full APK's signing certificates, as JSON, read by Android's own checks. */
    static String certs(Context c, File apk) {
        JSONArray a = new JSONArray();
        try {
            PackageInfo pi = c.getPackageManager().getPackageArchiveInfo(apk.getPath(), PackageManager.GET_SIGNING_CERTIFICATES);
            if (pi != null && pi.signingInfo != null)
                for (android.content.pm.Signature s : pi.signingInfo.getApkContentsSigners()) a.put(hex(sha256(s.toByteArray())));
        } catch (Exception ignored) {}
        return a.toString();
    }

    static byte[] sha256(byte[] b) throws Exception { return MessageDigest.getInstance("SHA-256").digest(b); }

    static String hex(byte[] b) {
        StringBuilder h = new StringBuilder();
        for (byte x : b) h.append(String.format("%02x", x));
        return h.toString();
    }

    static void copy(File from, File to) throws Exception {
        try (InputStream in = new java.io.FileInputStream(from); OutputStream out = new FileOutputStream(to)) {
            byte[] buf = new byte[1 << 16]; int n;
            while ((n = in.read(buf)) > 0) out.write(buf, 0, n);
        }
    }

    static void wipe(File f) {
        File[] all = f.listFiles();
        if (all != null) for (File x : all) wipe(x);
        f.delete();
    }

    // ── ReVanced's patcher, loaded from Manager's APK ──

    /**
     * The patcher, its library (apksig, ApkUtils) and their Kotlin runtime, from Manager's APK, in
     * a class loader of their own (parent: Android's boot classes only), driven by reflection.
     * Written against ReVanced Patcher 22 / Library 4 (Manager 2.6.0); if a later Manager changes
     * this API, patching stops with an error and the pinned Twitch stays as it is.
     */
    static final class Rv {
        final ClassLoader cl;
        final Object unit;

        Rv(Context c, File managerApk) throws Exception {
            managerApk.setWritable(false, false);           // Android 14+ refuses writable code files
            File opt = new File(c.getCodeCacheDir(), "revanced"); opt.mkdirs();
            cl = new DexClassLoader(managerApk.getPath(), opt.getPath(), null, ClassLoader.getSystemClassLoader().getParent());
            unit = cl.loadClass("kotlin.Unit").getField("INSTANCE").get(null);
        }

        /** A Kotlin lambda (kotlin.jvm.functions.FunctionN) backed by `body`. */
        Object fn(int arity, java.util.function.Function<Object[], Object> body) throws Exception {
            Class<?> f = cl.loadClass("kotlin.jvm.functions.Function" + arity);
            InvocationHandler h = (proxy, m, args) -> {
                switch (m.getName()) {
                    case "invoke": { Object r = body.apply(args == null ? new Object[0] : args); return r == null ? unit : r; }
                    case "hashCode": return System.identityHashCode(proxy);
                    case "equals": return proxy == args[0];
                    case "toString": return "Thurea lambda";
                    default: return null;
                }
            };
            return Proxy.newProxyInstance(cl, new Class<?>[]{ f }, h);
        }

        Object call(Object target, String name, Object... args) throws Exception {
            Class<?> k = target instanceof Class ? (Class<?>) target : target.getClass();
            for (Method m : k.getMethods())
                if (m.getName().equals(name) && m.getParameterTypes().length == args.length) {
                    m.setAccessible(true);
                    try { return m.invoke(target instanceof Class ? null : target, args); }
                    catch (java.lang.reflect.InvocationTargetException e) {
                        Throwable t = e.getCause() == null ? e : e.getCause();
                        if (t instanceof Exception) throw (Exception) t;
                        throw new Exception(t);
                    }
                }
            throw new Exception("ReVanced's patcher has no " + k.getSimpleName() + "." + name + " — this Thurea needs an update");
        }

        /**
         * The SHA-256s of an APK's signing certificates as JSON, if apksig (from Manager's APK)
         * verifies the whole file; else what Android's own parser reads ("[]" if neither can).
         */
        String verifiedCerts(Context c, File apk) {
            JSONArray a = new JSONArray();
            try {
                Class<?> b = cl.loadClass("com.android.apksig.ApkVerifier$Builder");
                Object builder = b.getConstructor(File.class).newInstance(apk);
                Object result = call(call(builder, "build"), "verify");
                if (!(Boolean) call(result, "isVerified")) return a.toString();
                for (Object cert : (List<?>) call(result, "getSignerCertificates")) a.put(hex(sha256(((X509Certificate) cert).getEncoded())));
                return a.toString();
            } catch (Exception ignored) {}
            return certs(c, apk);
        }

        /** The patches to use on Twitch: those on by default that name Twitch. */
        Set<Object> twitchPatches(File rvp) throws Exception {
            Object onFail = fn(2, a -> null);
            Class<?> loader = cl.loadClass("app.revanced.patcher.patch.Patch_androidKt");
            Collection<?> all = (Collection<?>) call(loader, "loadPatches", new File[]{ rvp }, onFail);
            Set<Object> out = new java.util.LinkedHashSet<>();
            for (Object p : all) {
                if (!(Boolean) call(p, "getUse")) continue;
                if (versions(p) != null) out.add(p);
            }
            if (out.isEmpty()) throw new Exception("ReVanced's patches have no Twitch patches");
            return out;
        }

        /**
         * The Twitch versions a patch names: an empty set for "any version", null if it isn't a
         * Twitch patch. Patcher 22 keeps compatible packages as Kotlin Pairs (name, versions or null).
         */
        Set<String> versions(Object patch) throws Exception {
            Object packages = call(patch, "getCompatiblePackages");
            if (packages == null) return null;          // universal patches: not chosen automatically
            for (Object pkg : (Collection<?>) packages) {
                Object name = call(pkg, "getFirst"), vs = call(pkg, "getSecond");
                if (!TWITCH.equals(name)) continue;
                Set<String> s = new TreeSet<>();
                if (vs != null) for (Object v : (Collection<?>) vs) s.add(String.valueOf(v));
                return s;
            }
            return null;
        }

        /** The Twitch versions every chosen patch supports, as a JSON array ([] means any). */
        JSONArray compatible(Set<Object> patches) throws Exception {
            Set<String> common = null;
            for (Object p : patches) {
                Set<String> v = versions(p);
                if (v == null || v.isEmpty()) continue;
                if (common == null) common = new TreeSet<>(v); else common.retainAll(v);
            }
            JSONArray a = new JSONArray();
            if (common != null) for (String v : common) a.put(v);
            return a;
        }

        /** Runs the patches on `apk`; writes the (unsigned) result to `out`. Returns the failures. */
        List<String> patch(Context c, File apk, Set<Object> patches, File out) throws Exception {
            File tmp = new File(work(c), "tmp"); wipe(tmp); tmp.mkdirs();
            File framework = new File(c.getCacheDir(), "revanced-framework"); framework.mkdirs();
            File aapt = new File(c.getApplicationInfo().nativeLibraryDir, "libaapt2.so");
            if (!aapt.isFile()) throw new Exception("this Thurea build has no aapt2 for " + Build.SUPPORTED_ABIS[0]);
            Class<?> k = cl.loadClass("app.revanced.patcher.PatchingKt");
            Object getPatches = fn(2, a -> patches);
            Object run = call(k, "patcher", apk, tmp, framework, aapt.getPath(), getPatches);
            List<String> failed = new ArrayList<>();
            Object onResult = fn(1, a -> {
                try {
                    Object ex = call(a[0], "getException");
                    if (ex != null) failed.add(call(call(a[0], "getPatch"), "getName") + ": " + ((Throwable) ex).getMessage());
                } catch (Exception ignored) {}
                return null;
            });
            Object result = call(run, "invoke", onResult);
            copy(apk, out);
            Object utils = cl.loadClass("app.revanced.library.ApkUtils").getField("INSTANCE").get(null);
            call(utils, "applyTo", result, out);
            wipe(tmp);
            return failed;
        }

        /**
         * Signs with this phone's own key (made on first use, kept in Thurea's private storage, never
         * leaves the phone). Every patched Twitch is signed with it, so updates install in place.
         */
        void sign(Context c, File in, File out) throws Exception {
            SharedPreferences p = prefs(c);
            String pass = p.getString("keystore.pass", null);
            if (pass == null) {
                byte[] r = new byte[24]; new SecureRandom().nextBytes(r);
                pass = hex(r);
                p.edit().putString("keystore.pass", pass).commit();
            }
            File ks = new File(c.getFilesDir(), "patched-apps.keystore");
            Class<?> d = cl.loadClass("app.revanced.library.ApkUtils$KeyStoreDetails");
            Object details = d.getConstructor(File.class, String.class, String.class, String.class).newInstance(ks, pass, "thurea", pass);
            Object utils = cl.loadClass("app.revanced.library.ApkUtils").getField("INSTANCE").get(null);
            call(utils, "signApk", in, out, "Thurea", details);
            if (!out.isFile()) throw new Exception("signing produced nothing");
        }
    }
}
