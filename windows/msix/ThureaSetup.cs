// Thurea Setup: the app inside thurea-windows.msix. It only asks for an Administrator and runs
// Setup.ps1 (next to it in the package), which installs Thurea outside the package.
using System;
using System.ComponentModel;
using System.Diagnostics;
using System.IO;
using System.Security.Principal;
using System.Windows.Forms;

static class ThureaSetup
{
    [STAThread]
    static int Main()
    {
        string dir = AppDomain.CurrentDomain.BaseDirectory;
        string ps = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.System), @"WindowsPowerShell\v1.0\powershell.exe");
        var psi = new ProcessStartInfo(ps, "-NoProfile -ExecutionPolicy Bypass -File \"" + Path.Combine(dir, "Setup.ps1") + "\"");
        psi.UseShellExecute = true;
        if (!new WindowsPrincipal(WindowsIdentity.GetCurrent()).IsInRole(WindowsBuiltInRole.Administrator)) psi.Verb = "runas";
        try { Process.Start(psi); return 0; }
        catch (Win32Exception)
        {
            MessageBox.Show("Thurea wasn't installed: it needs an Administrator to say yes.", "Thurea Setup", MessageBoxButtons.OK, MessageBoxIcon.Information);
            return 1;
        }
    }
}
