import QtQuick
import qs.Commons
import qs.Commons as Commons
import "ThureaGeometry.js" as Geo

// Thurea's logo, drawn by itself when `play()` is called: the outline traces round, the fill
// comes in, then the check mark. `progress` 1 shows it finished (no animation).
Canvas {
  id: root

  property real progress: 1
  // Colours: the logo's own blue by default; a single tint (e.g. the bar's text colour) when set.
  property color tint: "transparent"
  property bool warning: false
  property color warningColor: Commons.Color.urgent

  readonly property bool tinted: tint.a > 0

  implicitWidth: 64
  implicitHeight: 64

  function play() {
    anim.stop()
    progress = 0
    anim.start()
  }

  NumberAnimation on progress {
    id: anim
    running: false
    from: 0; to: 1
    duration: Style.duration(1400)
  }

  onProgressChanged: requestPaint()
  onTintChanged: requestPaint()
  onWarningChanged: requestPaint()
  onWidthChanged: requestPaint()
  onHeightChanged: requestPaint()

  function path(ctx, pts) {
    ctx.beginPath()
    ctx.moveTo(pts[0][0], pts[0][1])
    for (var i = 1; i < pts.length; i++) ctx.lineTo(pts[i][0], pts[i][1])
  }

  onPaint: {
    var ctx = getContext("2d")
    ctx.reset()
    var s = Math.min(width, height) / 58
    ctx.translate((width - 56 * s) / 2 - 26 * s, (height - 56 * s) / 2 - 26.5 * s)
    ctx.scale(s, s)
    var ph = Geo.phases(progress)
    var main = warning ? warningColor : (tinted ? tint : "#8CC8FF")

    if (ph.fill > 0) {
      path(ctx, Geo.outline)
      ctx.closePath()
      if (tinted || warning) {
        ctx.globalAlpha = ph.fill
        ctx.fillStyle = main
      } else {
        var g = ctx.createLinearGradient(31, 27, 77, 82)
        g.addColorStop(0, "#A9D8FF")
        g.addColorStop(1, "#4F86E0")
        ctx.globalAlpha = ph.fill
        ctx.fillStyle = g
      }
      ctx.fill()
      ctx.globalAlpha = 1
    }
    if (ph.fill < 1) {
      path(ctx, Geo.trim(Geo.outline, ph.outline))
      ctx.strokeStyle = main
      ctx.lineWidth = 2.6
      ctx.lineJoin = "round"
      ctx.lineCap = "round"
      ctx.stroke()
    }
    if (ph.check > 0) {
      path(ctx, Geo.trim(Geo.check, ph.check))
      ctx.strokeStyle = tinted || warning ? Commons.Color.background : "#111318"
      ctx.lineWidth = 5
      ctx.lineJoin = "round"
      ctx.lineCap = "round"
      ctx.stroke()
    }
  }
}
