import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import qs.Commons
import qs.Commons as Commons
import qs.Ui

// Thurea in the Omarchy bar: whether this computer is protected, the verse of the day, phone
// setup by QR code, and Thurea's settings. Everything comes from the `thurea` command (Thurea's
// Rust agent, /usr/bin/thurea), so the plugin itself holds no logic and no secrets.
Panel {
  id: root
  moduleName: "thurea.omarchy"
  ipcTarget: "thurea.omarchy"
  manageIpc: false

  Commons.Translations { id: strings; directory: Qt.resolvedUrl("i18n") }

  readonly property string thureaBin: "thurea"
  readonly property color foreground: bar ? bar.foreground : Commons.Color.foreground
  readonly property color urgent: bar ? bar.urgent : Commons.Color.urgent
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // From `thurea desktop-status`, `thurea verse --json` and `thurea prefs --json`.
  property var status: ({ state: "unknown", line: "", checks: {} })
  property var verse: ({ ref: "", text: "", translation: "WEB", notice: "" })
  property var prefs: ({ verses: true, animation: true })
  readonly property bool isProtected: status.state === "ok"
  property string actionText: ""
  // The panel shows Thurea (status, verse, actions) or, behind the gear, its settings.
  property bool settingsOpen: false

  function parseJson(text, fallback) {
    try { var v = JSON.parse(String(text || "")); return v && typeof v === "object" ? v : fallback } catch (e) { return fallback }
  }
  function refresh() {
    if (!statusProc.running) statusProc.running = true
    if (!verseProc.running) verseProc.running = true
    if (!prefsProc.running) prefsProc.running = true
  }
  function setPref(key, on) {
    var p = Object.assign({}, prefs); p[key] = on; prefs = p      // flips at once; the file follows
    setProc.command = [thureaBin, "prefs", "set", key, on ? "on" : "off"]
    setProc.running = true
  }
  function reapply() {
    actionText = strings.t("action.reapplying")
    reapplyProc.running = true
  }

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  onOpenedChanged: if (opened) {
    refresh()
    actionText = ""
    settingsOpen = false
    Qt.callLater(function() {
      if (panel.loadedContent) panel.loadedContent.forceActiveFocus()
      if (panel.loadedContent && root.prefs.animation !== false) panel.loadedContent.heroMark.play()
    })
  }

  Component.onCompleted: refresh()

  Timer { interval: 30000; running: true; repeat: true; onTriggered: if (!statusProc.running) statusProc.running = true }

  Process {
    id: statusProc
    command: [root.thureaBin, "desktop-status"]
    stdout: StdioCollector { waitForEnd: true; onStreamFinished: root.status = root.parseJson(text, { state: "unknown", line: strings.t("status.notInstalled"), checks: {} }) }
    onExited: function(code) { if (code !== 0) root.status = { state: "unknown", line: strings.t("status.notInstalled"), checks: {} } }
  }
  Process {
    id: verseProc
    command: [root.thureaBin, "verse", "--json"]
    stdout: StdioCollector { waitForEnd: true; onStreamFinished: root.verse = root.parseJson(text, root.verse) }
  }
  Process {
    id: prefsProc
    command: [root.thureaBin, "prefs", "--json"]
    stdout: StdioCollector { waitForEnd: true; onStreamFinished: root.prefs = root.parseJson(text, root.prefs) }
  }
  Process { id: setProc }
  Process {
    id: reapplyProc
    command: ["pkexec", "systemctl", "start", "thurea.service"]
    onExited: function(code) {
      root.actionText = code === 0 ? strings.t("action.reapplied") : strings.t("action.reapplyFailed")
      statusProc.running = true
    }
  }
  Process { id: openProc }

  ShellIpc {
    target: root.ipcTarget
    function open(): void { root.open() }
    function close(): void { root.close() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function toggle(): void { root.toggle() }
    function setupPhone(): void { phone.show("reset") }
    function keepPhone(): void { phone.show("keep") }
    function closePhone(): void { phone.hide() }
    function settings(): void { root.open(); root.settingsOpen = true }
    function family(): void { openProc.command = [root.thureaBin, "control-center"]; openProc.running = true }
    function status(): string { return root.isProtected ? strings.t("status.protected") : (root.status.line || "") }
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    iconComponent: Component {
      Item {
        implicitWidth: button.iconSize
        implicitHeight: button.iconSize
        ThureaMark {
          anchors.centerIn: parent
          width: button.iconSize * 1.05
          height: button.iconSize * 1.05
          tint: root.barForeground
          warning: !root.isProtected && root.status.state !== "unknown"
          opacity: root.status.state === "unknown" ? 0.55 : 1
        }
      }
    }
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.MiddleButton) phone.show()
      else root.toggle()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: panel.loadedContent
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(panel.loadedContentHeight, Style.space(640))

    content: PanelKeyCatcher {
      id: keys
      property alias heroMark: heroMark
      implicitHeight: column.implicitHeight
      anchors.fill: parent
      onCloseRequested: root.settingsOpen ? root.settingsOpen = false : root.close()
      onTabRequested: function(direction) { root.switchPanel(direction) }
      onTextKey: function(t) {
        if (t === "p" || t === "P") phone.show()
        else if (t === "r" || t === "R") root.reapply()
        else if (t === "s" || t === "S") root.settingsOpen = !root.settingsOpen
      }

      Flickable {
        anchors.fill: parent
        contentWidth: width
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        interactive: contentHeight > height
        ScrollBar.vertical: ScrollBar { policy: ScrollBar.AsNeeded }

        Column {
          id: column
          width: parent.width
          spacing: Style.space(12)

          // ── Hero: the logo draws itself as the panel opens ──
          RowLayout {
            visible: !root.settingsOpen
            width: parent.width
            spacing: Style.space(14)
            ThureaMark {
              id: heroMark
              Layout.preferredWidth: Style.space(56)
              Layout.preferredHeight: Style.space(56)
              warning: !root.isProtected && root.status.state !== "unknown"
            }
            ColumnLayout {
              Layout.fillWidth: true
              spacing: Style.space(2)
              PlainText { text: "Thurea"; font.pixelSize: Style.font.title; font.bold: true }
              PlainText {
                Layout.fillWidth: true
                text: root.isProtected ? strings.t("status.protected") : (root.status.line || strings.t("status.checking"))
                color: root.isProtected ? root.foreground : root.urgent
                font.pixelSize: Style.font.bodySmall
                wrapMode: Text.WordWrap
              }
              PlainText {
                visible: !!root.status.version
                text: strings.t("status.version").arg(root.status.version || "")
                color: root.dim
                font.pixelSize: Style.font.caption
              }
            }
            PanelActionButton {
              Layout.alignment: Qt.AlignTop
              iconText: "\u{f0493}"
              tooltipText: strings.t("section.settings")
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.settingsOpen = true
            }
          }

          // ── Settings header: back to Thurea ──
          RowLayout {
            visible: root.settingsOpen
            width: parent.width
            spacing: Style.space(8)
            PanelActionButton {
              iconText: "\u{f004d}"
              tooltipText: strings.t("settings.back")
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.settingsOpen = false
            }
            PlainText { Layout.fillWidth: true; text: strings.t("section.settings"); font.pixelSize: Style.font.title; font.bold: true }
          }

          // ── Verse of the day ──
          BorderSurface {
            visible: !root.settingsOpen && root.prefs.verses !== false && root.verse.text !== ""
            width: parent.width
            implicitHeight: verseCol.implicitHeight + Style.space(24)
            radius: Style.cornerRadius
            color: Style.hoverFillFor(root.foreground, Commons.Color.accent)
            borderSpec: Border.controlSpec("normal", root.foreground, Commons.Color.accent)
            Column {
              id: verseCol
              anchors.left: parent.left; anchors.right: parent.right; anchors.verticalCenter: parent.verticalCenter
              anchors.leftMargin: Style.space(14); anchors.rightMargin: Style.space(14)
              spacing: Style.space(8)
              PlainText { text: strings.t("verse.heading"); color: Commons.Color.accent; font.pixelSize: Style.font.caption; font.bold: true; font.letterSpacing: 1.2 }
              PlainText {
                width: parent.width
                text: "“" + root.verse.text + "”"
                font.pixelSize: Style.font.subtitle
                font.italic: true
                wrapMode: Text.WordWrap
                lineHeight: 1.25
              }
              PlainText {
                width: parent.width
                text: root.verse.ref + " · " + (root.verse.translation || "WEB")
                color: root.dim
                font.pixelSize: Style.font.caption
                horizontalAlignment: Text.AlignRight
              }
            }
          }

          // ── Actions ──
          RowLayout {
            visible: !root.settingsOpen
            width: parent.width
            spacing: Style.space(8)
            Button {
              Layout.fillWidth: true
              text: strings.t("action.setupPhone")
              iconText: "\u{f011c}"
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: { root.close(); phone.show() }
            }
            Button {
              text: strings.t("action.family")
              iconText: "\u{f0c0}"
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: { root.close(); openProc.command = [root.thureaBin, "control-center"]; openProc.running = true }
            }
            Button {
              text: strings.t("action.reapply")
              iconText: "\u{f0450}"
              bordered: true
              foreground: root.foreground
              fontFamily: root.fontFamily
              onClicked: root.reapply()
            }
          }
          PlainText { visible: !root.settingsOpen && root.actionText !== ""; width: parent.width; text: root.actionText; color: root.dim; font.pixelSize: Style.font.caption; wrapMode: Text.WordWrap }

          // ── Settings, behind the gear (shared with Thurea's other desktop apps:
          //    ~/.config/thurea/prefs.json) ──
          Toggle {
            visible: root.settingsOpen
            width: parent.width
            label: strings.t("settings.verses")
            description: strings.t("settings.versesHelp")
            checked: root.prefs.verses !== false
            foreground: root.foreground
            fontFamily: root.fontFamily
            onClicked: root.setPref("verses", !checked)
          }
          Toggle {
            visible: root.settingsOpen
            width: parent.width
            label: strings.t("settings.animation")
            description: strings.t("settings.animationHelp")
            checked: root.prefs.animation !== false
            foreground: root.foreground
            fontFamily: root.fontFamily
            onClicked: root.setPref("animation", !checked)
          }

          PlainText {
            visible: root.settingsOpen
            width: parent.width
            text: strings.t("credits.line")
            color: root.dim
            font.pixelSize: Style.font.caption
            wrapMode: Text.WordWrap
            MouseArea {
              anchors.fill: parent
              cursorShape: Qt.PointingHandCursor
              onClicked: { openProc.command = ["xdg-open", "/usr/share/thurea/CREDITS.md"]; openProc.running = true }
            }
          }
        }
      }
    }
  }

  // ── Phone setup: the QR code a new or factory-reset Android phone scans on its Welcome
  //    screen (tap it 6 times). `thurea setup-phone --tray` serves Thurea to the phone from this
  //    computer while the code is up, and stops when it closes. ──
  Item {
    id: phone

    property bool shown: false
    // "reset": a new or factory-reset phone scans the Welcome-screen code (setup-phone).
    // "keep": a phone in use, nothing erased; it pairs by Wireless debugging (adopt-phone).
    property string mode: "reset"
    property bool connected: false
    property bool done: false
    property bool withWifi: true
    property var rows: []
    property int size: 0
    property string error: ""
    property string progress: ""
    property string wifiName: ""
    property bool household: false
    property bool expectedStop: false
    property var pending: []

    readonly property color text: Commons.Color.popups.text
    readonly property color dim: Qt.darker(Commons.Color.popups.text, 1.5)

    function show(m) {
      if (m === "keep" || m === "reset") mode = m
      shown = true
      start()
      Qt.callLater(function() { if (phone.shown) phoneKeys.forceActiveFocus() })
    }
    function hide() {
      shown = false
      if (qrProc.running) { expectedStop = true; qrProc.running = false }
      rows = []; size = 0; error = ""; progress = ""
    }
    function start() {
      if (qrProc.running) { expectedStop = true; qrProc.running = false; Qt.callLater(start); return }
      rows = []; pending = []; size = 0; error = ""; progress = strings.t("phone.preparing"); wifiName = ""
      connected = false; done = false
      expectedStop = false
      var cmd
      if (mode === "keep") cmd = [root.thureaBin, "adopt-phone", "--tray"]
      else {
        cmd = [root.thureaBin, "setup-phone", "--tray"]
        if (withWifi) cmd.push("--wifi-current")
      }
      qrProc.command = cmd
      qrProc.running = true
    }
    function line(l) {
      var f = String(l).split("\t")
      if (f[0] === "m" && f.length > 1 && /^[01]+$/.test(f[1])) pending.push(f[1])
      else if (f[0] === "ready") {
        if (pending.length > 0 && pending.length === pending[0].length) { rows = pending; size = pending.length }
        else error = strings.t("phone.badCode")
        household = f[3] === "household"
        wifiName = f[4] || ""
        progress = strings.t(mode === "keep" ? "keep.waiting" : "phone.waiting")
      } else if (f[0] === "event") {
        var ev = { downloading: "phone.downloading", downloaded: "phone.downloaded", retry: "phone.retry",
                   joined: "phone.joined", finished: "phone.finished", expired: "phone.expired",
                   found: "keep.found", paired: "keep.paired", connected: "keep.connected", phone: "keep.phone",
                   installing: "keep.installing", installed: "keep.installed", signout: "keep.signout",
                   "signout-wait": "keep.signout", owner: "keep.owner", "owner-failed": "keep.ownerFailed",
                   signin: "keep.signin", "signin-samsung": "keep.signinSamsung", signedin: "keep.signedin",
                   "signin-later": "keep.signinLater", apps: "keep.apps", done: "keep.done" }[f[1]]
        if (["connected", "phone", "installing", "installed"].indexOf(f[1]) >= 0) connected = true
        if (f[1] === "done" || f[1] === "finished" || f[1] === "joined") done = true
        if (f[1] === "done" && f[2] === "updated") ev = "keep.updated"
        if (ev) progress = strings.t(ev).arg(f[2] || "")
      } else if (f[0] === "error") error = f.slice(1).join(" ")
    }

    Process {
      id: qrProc
      stdout: SplitParser { onRead: function(data) { if (!phone.expectedStop) phone.line(data) } }
      stderr: StdioCollector { waitForEnd: true; onStreamFinished: if (!phone.expectedStop && phone.error === "" && phone.size === 0 && String(text).trim() !== "") phone.error = String(text).trim().split("\n").pop() }
      onExited: function(code) { if (!phone.expectedStop && code !== 0 && phone.error === "") phone.error = strings.t("phone.failed") }
    }

    OverlayWindow {
      shown: phone.shown
      WlrLayershell.namespace: "thurea-phone-setup"

      Rectangle {
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.72)
        MouseArea { anchors.fill: parent; onClicked: phone.hide() }
      }

      Item {
        id: phoneKeys
        anchors.fill: parent
        focus: true
        Keys.onEscapePressed: phone.hide()

        // A solid card in the theme's popup colours, so nothing behind shows through the text.
        BorderSurface {
          id: cardFrame
          readonly property int pad: Style.space(24)
          anchors.centerIn: parent
          width: Style.space(400)
          height: card.implicitHeight + pad * 2
          radius: Style.cornerRadius
          color: Commons.Color.popups.background
          borderSpec: Border.surfaceSpec("popups", "border", Commons.Color.popups.border, Math.max(1, Style.space(2)))
          scale: Math.min(1, (phoneKeys.width - Style.space(32)) / Math.max(1, width), (phoneKeys.height - Style.space(32)) / Math.max(1, height))
          MouseArea { anchors.fill: parent; onClicked: {} }

          ColumnLayout {
            id: card
            anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
            anchors.margins: cardFrame.pad
            spacing: Style.space(16)

            // ── Title ──
            RowLayout {
              Layout.fillWidth: true
              spacing: Style.space(12)
              ThureaMark { id: phoneMark; Layout.preferredWidth: Style.space(36); Layout.preferredHeight: Style.space(36) }
              ColumnLayout {
                Layout.fillWidth: true
                spacing: Style.space(2)
                CardText { text: strings.t("phone.heading"); font.pixelSize: Style.font.heading; font.bold: true }
                CardText { text: strings.t(phone.mode === "keep" ? "keep.subheading" : "phone.subheading"); color: phone.dim; font.pixelSize: Style.font.caption }
              }
              PanelActionButton {
                Layout.alignment: Qt.AlignTop
                iconText: "\u{f0156}"
                tooltipText: strings.t("phone.close")
                foreground: phone.text
                fontFamily: root.fontFamily
                onClicked: phone.hide()
              }
            }
            Connections { target: phone; function onShownChanged() { if (phone.shown && root.prefs.animation !== false) phoneMark.play() } }

            // ── Which kind of setup ──
            RowLayout {
              Layout.fillWidth: true
              spacing: Style.space(8)
              Repeater {
                model: [{ id: "reset", label: strings.t("mode.reset") }, { id: "keep", label: strings.t("mode.keep") }]
                Button {
                  required property var modelData
                  Layout.fillWidth: true
                  text: modelData.label
                  selected: phone.mode === modelData.id
                  bordered: true
                  foreground: phone.text
                  fontFamily: root.fontFamily
                  onClicked: if (phone.mode !== modelData.id) { phone.mode = modelData.id; phone.start() }
                }
              }
            }

            // ── The code: one native rectangle per module (crisp), as Omarchy's Wi-Fi QR does ──
            Rectangle {
              id: qrCanvas
              readonly property int moduleSize: phone.size > 0 ? Math.max(3, Math.floor(Style.space(260) / phone.size)) : 0
              visible: phone.size > 0 && phone.error === "" && !phone.connected
              width: phone.size * moduleSize
              height: width
              color: "white"
              radius: Style.cornerRadius
              Layout.alignment: Qt.AlignHCenter
              Grid {
                anchors.fill: parent
                columns: phone.size
                LayoutMirroring.enabled: false
                Repeater {
                  model: phone.size * phone.size
                  Rectangle {
                    required property int index
                    width: qrCanvas.moduleSize; height: qrCanvas.moduleSize
                    color: (phone.rows[Math.floor(index / phone.size)] || "").charAt(index % phone.size) === "1" ? "#111111" : "transparent"
                  }
                }
              }
            }
            CardText {
              visible: phone.error !== ""
              text: phone.error
              color: Commons.Color.urgent
              horizontalAlignment: Text.AlignHCenter
            }

            // ── Status ──
            RowLayout {
              Layout.fillWidth: true
              spacing: Style.space(10)
              Rectangle {
                id: dot
                Layout.preferredWidth: Style.space(8); Layout.preferredHeight: Style.space(8)
                radius: width / 2
                color: Commons.Color.accent
                SequentialAnimation on opacity {
                  running: phone.shown && phone.error === "" && !phone.done
                  loops: Animation.Infinite
                  NumberAnimation { to: 0.3; duration: Style.duration(700); easing.type: Easing.InOutSine }
                  NumberAnimation { to: 1; duration: Style.duration(700); easing.type: Easing.InOutSine }
                }
              }
              CardText { text: phone.progress; font.bold: true }
            }

            PanelSeparator { foreground: phone.text; Layout.fillWidth: true }

            // ── Steps ──
            ColumnLayout {
              Layout.fillWidth: true
              spacing: Style.space(10)
              Repeater {
                model: phone.mode === "keep"
                  ? [strings.t("keep.step1"), strings.t("keep.step2"), strings.t("keep.step3")]
                  : [strings.t("phone.step1"), strings.t("phone.step2"), strings.t("phone.step3")]
                RowLayout {
                  required property var modelData
                  required property int index
                  Layout.fillWidth: true
                  spacing: Style.space(12)
                  Rectangle {
                    Layout.preferredWidth: Style.space(22); Layout.preferredHeight: Style.space(22)
                    Layout.alignment: Qt.AlignTop
                    radius: width / 2
                    color: "transparent"
                    border.width: 1
                    border.color: Commons.Color.accent
                    CardText {
                      anchors.centerIn: parent
                      width: implicitWidth
                      text: String(index + 1)
                      color: Commons.Color.accent
                      font.pixelSize: Style.font.caption
                      font.bold: true
                    }
                  }
                  CardText { text: modelData; font.pixelSize: Style.font.bodySmall; lineHeight: 1.15 }
                }
              }
            }

            // ── Wi-Fi in the code ──
            Toggle {
              visible: phone.mode === "reset"
              Layout.fillWidth: true
              label: phone.wifiName !== "" ? strings.t("phone.wifiLabel").arg(phone.wifiName) : strings.t("phone.wifiLabelNone")
              description: phone.withWifi
                ? (phone.wifiName !== "" ? strings.t("phone.wifiOnShort") : strings.t("phone.wifiNoneShort"))
                : strings.t("phone.wifiOffShort")
              checked: phone.withWifi
              foreground: phone.text
              fontFamily: root.fontFamily
              onClicked: { phone.withWifi = !phone.withWifi; phone.start() }
            }

            CardText {
              text: phone.mode === "keep" ? strings.t("keep.note") : phone.household ? strings.t("phone.household") : strings.t("phone.standalone")
              color: phone.dim
              font.pixelSize: Style.font.caption
            }
          }
        }
      }
    }
  }

  component CardText: Text {
    textFormat: Text.PlainText
    color: phone.text
    font.family: root.fontFamily
    font.pixelSize: Style.font.body
    wrapMode: Text.WordWrap
    Layout.fillWidth: true
  }

  component PlainText: Text {
    textFormat: Text.PlainText
    color: root.foreground
    font.family: root.fontFamily
    font.pixelSize: Style.font.body
  }
}
