import QtQuick

// InputSignal keeps the recent loudness of one input, whether it is speech,
// and whether audio still arrives. Samples get evenly spaced times (one frame
// apart) rather than arrival times, so a trace can scroll smoothly even when
// the socket delivers them in bursts.
QtObject {
  id: input

  // One sample per 32 ms frame; about five seconds kept.
  readonly property int frameMs: 32
  readonly property int capacity: 160
  property var levels: []
  property var times: []
  // Index of levels[0] since the input started, so a sample keeps its parity.
  property int firstIndex: 0
  property bool speech: false
  property double lastAt: 0
  // No frame for a second means the device stopped sending audio.
  readonly property bool alive: clock.now - lastAt < 1000

  signal updated()

  function push(level, isSpeech) {
    const now = Date.now()
    const previous = times.length > 0 ? times[times.length - 1] : 0
    // Keep the even cadence unless it drifted too far from the real clock.
    const expected = previous + frameMs
    levels.push(level)
    times.push(Math.abs(expected - now) > 250 ? now : expected)
    if (levels.length > capacity) {
      levels.shift()
      times.shift()
      firstIndex += 1
    }
    speech = isSpeech
    lastAt = now
    updated()
  }

  property QtObject clock: QtObject {
    property double now: Date.now()
  }
  property Timer tick: Timer {
    interval: 500
    repeat: true
    running: true
    onTriggered: input.clock.now = Date.now()
  }
}
