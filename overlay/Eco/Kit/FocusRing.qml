import QtQuick
import Eco.Core

// FocusRing outlines the control the keyboard is on, apart from its own border,
// so it shows whatever state the control is in: two pixels of the accent, drawn
// inside the control so no container clips it and it reads at any screen scale.
Rectangle {
  property bool shown: parent.activeFocus
  anchors.fill: parent
  anchors.margins: 2
  z: 1
  color: "transparent"
  border.color: Theme.primary
  border.width: 2
  opacity: shown ? 1 : 0
  Behavior on opacity { NumberAnimation { duration: 110 } }
}
