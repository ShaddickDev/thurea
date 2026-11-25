import android.content.Intent;
import android.os.PersistableBundle;
import java.lang.reflect.Method;
import java.util.Iterator;
import org.json.JSONObject;

/**
 * Lab only: stands in for the setup wizard's QR scanner. Takes the text decoded from
 * Thurea's setup QR (by zbarimg, on the computer) and hands it to Android's own
 * provisioning app exactly as the setup wizard does after a scan. Run as root:
 *   CLASSPATH=/data/local/tmp/scan.dex app_process / Scan /data/local/tmp/qr.json
 * and, once provisioning is done, the setup wizard's last step:
 *   CLASSPATH=/data/local/tmp/scan.dex app_process / Scan --finalize
 */
public class Scan {
    public static void main(String[] a) throws Exception {
        if (a[0].equals("--finalize")) {   // what the setup wizard does as its last step
            start(new Intent("android.app.action.PROVISION_FINALIZATION").addFlags(Intent.FLAG_ACTIVITY_NEW_TASK));
            return;
        }
        JSONObject j = new JSONObject(new String(java.nio.file.Files.readAllBytes(java.nio.file.Paths.get(a[0]))));
        Intent i = new Intent("android.app.action.PROVISION_MANAGED_DEVICE_FROM_TRUSTED_SOURCE");
        i.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK);
        for (Iterator<String> it = j.keys(); it.hasNext();) {
            String k = it.next();
            Object v = j.get(k);
            if (v instanceof JSONObject) {
                JSONObject o = (JSONObject) v;
                PersistableBundle b = new PersistableBundle();
                for (Iterator<String> it2 = o.keys(); it2.hasNext();) { String kk = it2.next(); b.putString(kk, o.getString(kk)); }
                i.putExtra(k, b);
            } else if (k.endsWith("_COMPONENT_NAME")) {   // the setup wizard turns this text into a ComponentName
                i.putExtra(k, android.content.ComponentName.unflattenFromString(v.toString()));
            } else if (v instanceof Boolean) i.putExtra(k, (Boolean) v);
            else i.putExtra(k, v.toString());
        }
        i.putExtra("android.app.extra.PROVISIONING_TRIGGER", 2); // PROVISIONING_TRIGGER_QR_CODE
        start(i);
    }

    static void start(Intent i) throws Exception {
        Object am = Class.forName("android.app.ActivityManager").getMethod("getService").invoke(null);
        for (Method m : am.getClass().getMethods()) {
            if (m.getName().equals("startActivityAsUser") && m.getParameterCount() == 11) {
                Object r = m.invoke(am, null, "com.android.shell", i, null, null, null, 0, 0, null, null, 0);
                System.out.println("provisioning started (result " + r + ")");
                return;
            }
        }
        for (Method m : am.getClass().getMethods()) if (m.getName().startsWith("startActivity")) System.out.println(m);
        System.out.println("startActivityAsUser not found");
        System.exit(1);
    }
}
