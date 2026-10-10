import QtQuick
import QtQuick.Controls.Basic as C
import Eco.Core

// Hint is the hover tooltip: monospace text in a frame of the accent.
C.ToolTip {
  id: hint
  delay: 450
  padding: 8
  contentItem: Label {
    text: hint.text
    wrapMode: Text.Wrap
  }
  background: Rectangle {
    color: Theme.background
    border.color: Theme.primary
  }
}
