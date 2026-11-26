// Thurea's logo as polylines, so it can be drawn a little at a time (the same shape and the
// same timeline as the Android app's and the Windows tray's opening animation).
// Coordinates are the logo's own: x 31..77, y 27..82 (see extension/icons/thurea.svg).
.pragma library

function cubic(p0, p1, p2, p3, n, out) {
  for (var i = 1; i <= n; i++) {
    var t = i / n, u = 1 - t
    out.push([u*u*u*p0[0] + 3*u*u*t*p1[0] + 3*u*t*t*p2[0] + t*t*t*p3[0],
              u*u*u*p0[1] + 3*u*u*t*p1[1] + 3*u*t*t*p2[1] + t*t*t*p3[1]])
  }
}

// M54,27 L77,35.5 V52 C77,66.5 67.5,76.5 54,82 C40.5,76.5 31,66.5 31,52 V35.5 Z
var outline = (function() {
  var p = [[54, 27], [77, 35.5], [77, 52]]
  cubic([77, 52], [77, 66.5], [67.5, 76.5], [54, 82], 24, p)
  cubic([54, 82], [40.5, 76.5], [31, 66.5], [31, 52], 24, p)
  p.push([31, 35.5]); p.push([54, 27])
  return p
})()

var check = [[43.5, 54.5], [51, 62], [65, 47]]

function length(pts) {
  var l = 0
  for (var i = 1; i < pts.length; i++) l += Math.hypot(pts[i][0] - pts[i-1][0], pts[i][1] - pts[i-1][1])
  return l
}

// The first `t` (0..1) of a polyline, by length.
function trim(pts, t) {
  if (t >= 1) return pts
  if (t <= 0) return [pts[0]]
  var want = length(pts) * t, out = [pts[0]]
  for (var i = 1; i < pts.length; i++) {
    var a = pts[i-1], b = pts[i], d = Math.hypot(b[0] - a[0], b[1] - a[1])
    if (d >= want) { out.push([a[0] + (b[0] - a[0]) * want / d, a[1] + (b[1] - a[1]) * want / d]); return out }
    want -= d; out.push(b)
  }
  return out
}

function clamp01(x) { return Math.max(0, Math.min(1, x)) }
function easeOut(x) { return 1 - Math.pow(1 - x, 3) }

// Timeline (ms of 1400): outline 0-900, fill 750-1150, check 1050-1400.
function phases(progress) {
  var ms = progress * 1400
  return {
    outline: easeOut(clamp01(ms / 900)),
    fill: clamp01((ms - 750) / 400),
    check: easeOut(clamp01((ms - 1050) / 350))
  }
}

if (typeof module !== "undefined") module.exports = { outline: outline, check: check, trim: trim, length: length, phases: phases }
