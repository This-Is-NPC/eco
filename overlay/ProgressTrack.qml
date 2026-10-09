import QtQuick

// ProgressTrack is a frame filled from the left to `fraction`; the fill grows
// as the work it measures moves, never on its own.
Rectangle {
  id: track
  property real fraction: 0
  color: Qt.alpha(Theme.primary, 0.05)
  border.color: Qt.alpha(Theme.primary, 0.45)

  Rectangle {
    anchors { left: parent.left; top: parent.top; bottom: parent.bottom; margins: 1 }
    width: (track.width - 2) * Math.max(0, Math.min(1, track.fraction))
    color: Qt.alpha(Theme.primary, 0.12)
    Behavior on width { NumberAnimation { duration: 400; easing.type: Easing.OutCubic } }
  }
}
