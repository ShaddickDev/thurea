import QtQuick
import Quickshell
import Quickshell.Io

// Omarea's headless half: every 30 seconds `omarea check` compares Thurea's state with the last
// look and sends an Omarchy notification when something needs a parent (protection stopped, a
// family request is waiting, a child is nearly out of screen time). The logic is in `omarea`
// (shared with Omarchy 3's Waybar module), so this file only keeps the clock. It runs whenever
// the plugin is enabled, in the bar or not, and does nothing if Thurea isn't installed.
Item {
  id: root

  property var shell: null
  property var manifest: null
  property string omarchyPath: Quickshell.env("OMARCHY_PATH")

  function check() {
    if (!checkProc.running) checkProc.running = true
  }

  // The first look waits for the session to settle (the shell starts with Hyprland).
  Timer { interval: 20000; running: true; repeat: false; onTriggered: root.check() }
  Timer { interval: 30000; running: true; repeat: true; onTriggered: root.check() }

  Process {
    id: checkProc
    command: ["omarea", "check"]
  }
}
