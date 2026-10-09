import QtQuick

// LoadingTrace is the trace drawn while something loads: it moves in from the
// left toward the end but stops short of it while the work goes on; once the
// work is done it reaches the end, and goes.
Item {
  id: loading

  property bool running: false
  property color tone: Theme.primary
  // The colour behind the trace, which its left edge fades into.
  property color backdrop: Theme.background
  property real progress: 0
  property int frame: 0

  visible: running || finish.running
  clip: true

  // Start over, as when new work replaces the one in progress.
  function restart() {
    finish.stop()
    progress = 0
    fill.restart()
    if (body.item)
      body.item.seed()
  }

  function level(index) {
    return 0.12 + 0.62 * Math.abs(Math.sin(index * 0.29) * Math.cos(index * 0.11))
  }

  onRunningChanged: {
    if (running) {
      restart()
    } else {
      fill.stop()
      finish.restart()
    }
  }

  NumberAnimation { id: fill; target: loading; property: "progress"; to: 0.92; duration: 1800; easing.type: Easing.Linear }
  NumberAnimation { id: finish; target: loading; property: "progress"; to: 1; duration: 220; easing.type: Easing.OutCubic }

  // The trace exists only while it shows: a button at rest holds none.
  Loader {
    id: body
    anchors.fill: parent
    active: loading.visible
    sourceComponent: Item {
      // A full window of samples, so the trace enters already drawn.
      function seed() {
        const now = Date.now()
        const levels = []
        const times = []
        for (let i = 0; i < signal.capacity; i++) {
          levels.push(loading.level(i))
          times.push(now - (signal.capacity - 1 - i) * signal.frameMs)
        }
        loading.frame = signal.capacity
        signal.firstIndex = 0
        signal.levels = levels
        signal.times = times
        signal.speech = true
        signal.lastAt = now
        signal.updated()
      }

      Component.onCompleted: seed()

      InputSignal { id: signal }

      Timer {
        interval: signal.frameMs
        repeat: true
        running: true
        onTriggered: signal.push(loading.level(loading.frame++), true)
      }

      EcgTrace {
        x: loading.width * (loading.progress - 1)
        width: loading.width
        height: loading.height
        source: signal
        tone: loading.tone
        backdrop: loading.backdrop
        reach: 0.7
        glow: false
      }
    }
  }
}
