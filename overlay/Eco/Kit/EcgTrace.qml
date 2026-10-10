import QtQuick
import QtQuick.Shapes
import Eco.Core

// EcgTrace draws an input like a monitor trace: a flat line in silence that
// swings with the input's loudness, written by a bright head on the right and
// fading out to the left. Brighter while it hears speech; a faint dashed line
// when the input sends nothing.
//
// The trace is laid out once, in pieces of a few samples each, on a strip that
// slides left once per display frame by the time that passed. A new sample
// rebuilds only the newest piece; the others keep their geometry on the GPU,
// so it scrolls at the display's rate for little CPU.
Item {
  id: trace

  property InputSignal source: null
  property color tone: Theme.primary
  // How much of the height a full-scale swing may use.
  property real reach: 0.9
  property bool glow: true
  // The colour behind the trace, which the left edge fades into.
  property color backdrop: Theme.background
  readonly property bool alive: source !== null && source.alive
  // Drawing runs this far behind the newest sample, to ride out socket bursts.
  readonly property int delayMs: 70
  // Samples per piece: the most a new sample makes the CPU lay out again.
  readonly property int pieceSamples: 16

  readonly property real strength: source !== null && source.speech ? 1 : 0.6
  // When the strip's left edge was heard: positions on it never move.
  property double origin: 0
  property real shift: 0
  property real headY: height / 2
  // The samples drawn, as {index, x, y} on the strip, oldest first.
  property var raw: []
  // The pieces drawn, oldest first: {first, shape} — `first` is the index in
  // `raw` of the piece's first sample; a piece ends on the next one's first.
  property var pieces: []
  // When a frame was last drawn. Without frames — the window hidden, on
  // another workspace — samples are not laid out; the first frame back lays
  // the whole trace out again.
  property double drawnAt: 0
  property bool stale: false

  clip: true

  function span() { return source.capacity * source.frameMs }
  function at(time) { return (time - origin) / span() * width }

  // The point of sample `i` of a source, on the strip.
  function sample(i) {
    const index = source.firstIndex + i
    // Alternate up and down by the sample's own index, so it never flips sides.
    const sign = index % 2 === 0 ? 1 : -1
    const mid = height / 2
    return { index: index, x: at(source.times[i]), y: mid - sign * source.levels[i] * mid * reach }
  }

  // Lay the whole trace out again: a new source, size, or a gap in the samples.
  function rebuild() {
    for (const piece of pieces)
      piece.shape.destroy()
    pieces = []
    raw = []
    if (!alive || source.levels.length === 0)
      return
    origin = source.times[0]
    for (let i = 0; i < source.levels.length; i++)
      raw.push(sample(i))
    for (let first = 0; first < raw.length - 1; first += pieceSamples)
      pieces.push({ first: first, shape: pieceShape.createObject(strip) })
    for (let k = 0; k < pieces.length; k++)
      draw(k)
    slide()
  }

  // Take the newest sample: only the newest piece is laid out again.
  function append() {
    if (Date.now() - drawnAt > 250) {
      stale = true
      return
    }
    const last = source.levels.length - 1
    if (!alive || pieces.length === 0 || last < 0 || source.firstIndex + last !== raw[raw.length - 1].index + 1)
      return rebuild()
    raw.push(sample(last))
    let newest = pieces.length - 1
    if (raw.length - 1 - pieces[newest].first > pieceSamples) {
      // The newest piece is full: it ends where the next begins, and its last
      // stretch now knows what follows it.
      pieces.push({ first: raw.length - 2, shape: pieceShape.createObject(strip) })
      draw(newest)
      newest += 1
    }
    draw(newest)
    forget()
  }

  // Drop the samples and pieces that left the strip on the left; after a long
  // while the strip starts over, so its positions stay precise.
  function forget() {
    if (at(Date.now()) > 50 * width)
      return rebuild()
    const gone = at(Date.now() - delayMs) - width - 4
    while (pieces.length > 1 && raw[pieces[1].first].x < gone) {
      pieces[0].shape.destroy()
      const dropped = pieces[1].first
      pieces.shift()
      raw.splice(0, dropped)
      for (const piece of pieces)
        piece.first -= dropped
    }
  }

  // Lay out piece `k` as a curve through its samples: Catmull-Rom, two extra
  // points per stretch, reading one sample on each side for the tangents.
  function draw(k) {
    const first = pieces[k].first
    const end = k + 1 < pieces.length ? pieces[k + 1].first : raw.length - 1
    const smooth = []
    for (let i = first; i < end; i++) {
      const p0 = raw[Math.max(i - 1, 0)], p1 = raw[i], p2 = raw[i + 1], p3 = raw[Math.min(i + 2, raw.length - 1)]
      for (const t of [0, 1 / 3, 2 / 3]) {
        const t2 = t * t, t3 = t2 * t
        smooth.push(Qt.point(
          0.5 * (2 * p1.x + (-p0.x + p2.x) * t + (2 * p0.x - 5 * p1.x + 4 * p2.x - p3.x) * t2 + (-p0.x + 3 * p1.x - 3 * p2.x + p3.x) * t3),
          0.5 * (2 * p1.y + (-p0.y + p2.y) * t + (2 * p0.y - 5 * p1.y + 4 * p2.y - p3.y) * t2 + (-p0.y + 3 * p1.y - 3 * p2.y + p3.y) * t3)))
      }
    }
    smooth.push(Qt.point(raw[end].x, raw[end].y))
    pieces[k].shape.points = smooth
  }

  // Slide the strip by the time that passed, and keep the head on the trace.
  function slide() {
    if (!alive || raw.length < 2)
      return
    shift = width - 3 - at(Date.now() - delayMs)
    const edge = width - 3 - shift
    for (let i = raw.length - 1; i > 0; i--) {
      if (raw[i - 1].x <= edge) {
        const a = raw[i - 1], b = raw[i]
        const t = b.x === a.x ? 0 : Math.min(1, Math.max(0, (edge - a.x) / (b.x - a.x)))
        headY = a.y + (b.y - a.y) * t
        return
      }
    }
  }

  Connections {
    target: trace.source
    function onUpdated() { trace.append() }
  }
  onAliveChanged: rebuild()
  onWidthChanged: rebuild()
  onHeightChanged: rebuild()
  Component.onDestruction: {
    for (const piece of pieces)
      piece.shape.destroy()
  }

  FrameAnimation {
    running: trace.visible && trace.alive
    onTriggered: {
      trace.drawnAt = Date.now()
      if (trace.stale) {
        trace.stale = false
        trace.rebuild()
      }
      trace.slide()
    }
  }

  Item {
    id: strip
    anchors.fill: parent
    visible: trace.alive
    transform: Translate { x: trace.shift }
  }

  Component {
    id: pieceShape
    // Pieces meet end to end with flat caps: the curve's tangent runs on across
    // the joint, and no overlap draws a brighter dot there.
    Shape {
      property var points: []
      anchors.fill: parent
      preferredRendererType: Shape.CurveRenderer

      ShapePath {
        strokeWidth: trace.glow ? 5 : 0
        strokeColor: Qt.alpha(trace.tone, 0.12 * trace.strength)
        fillColor: "transparent"
        joinStyle: ShapePath.RoundJoin
        capStyle: ShapePath.FlatCap
        PathPolyline { path: points }
      }
      ShapePath {
        strokeWidth: 1.4
        strokeColor: Qt.alpha(trace.tone, trace.strength)
        fillColor: "transparent"
        joinStyle: ShapePath.RoundJoin
        capStyle: ShapePath.FlatCap
        PathPolyline { path: points }
      }
    }
  }

  // The older end fades into what lies behind.
  Rectangle {
    visible: trace.alive
    width: parent.width * 0.45
    height: parent.height
    gradient: Gradient {
      orientation: Gradient.Horizontal
      GradientStop { position: 0; color: trace.backdrop }
      GradientStop { position: 1; color: Qt.alpha(trace.backdrop, 0) }
    }
  }

  // The writing head.
  Rectangle {
    visible: trace.alive
    x: trace.width - 3 - width / 2
    y: trace.headY - height / 2
    width: 12
    height: 12
    radius: 6
    color: Qt.alpha(trace.tone, 0.18 * trace.strength)
    Rectangle {
      anchors.centerIn: parent
      width: 4.4
      height: 4.4
      radius: 2.2
      color: Qt.alpha(trace.tone, trace.strength)
    }
  }

  // No audio: a faint dashed line.
  Shape {
    anchors.fill: parent
    visible: !trace.alive
    ShapePath {
      strokeWidth: 1
      strokeColor: Qt.alpha(trace.tone, 0.35)
      strokeStyle: ShapePath.DashLine
      dashPattern: [2, 6]
      fillColor: "transparent"
      startX: 0
      startY: trace.height / 2
      PathLine { x: trace.width; y: trace.height / 2 }
    }
  }
}
