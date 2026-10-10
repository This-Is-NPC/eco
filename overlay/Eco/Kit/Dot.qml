import QtQuick
import Eco.Core

// Dot marks the chosen one of several: filled in its tone when it is, an
// outline when it could be.
Rectangle {
  property bool filled: true
  property color tone: Theme.primary
  property real size: 5
  implicitWidth: size
  implicitHeight: size
  radius: size / 2
  color: filled ? tone : "transparent"
  border.color: tone
}
