import QtQuick
import Eco.Core

// Kicker titles a group: a tracked label followed by a hairline to the edge.
Item {
  id: kicker
  property string text
  property color tone: Theme.dim
  implicitHeight: label.implicitHeight + 14
  Label {
    id: label
    anchors { left: parent.left; bottom: parent.bottom; bottomMargin: 4 }
    text: kicker.text.toUpperCase()
    color: kicker.tone
    font.pixelSize: 10
    font.letterSpacing: 3
  }
  Rectangle {
    anchors { left: label.right; leftMargin: 10; right: parent.right; verticalCenter: label.verticalCenter }
    height: 1
    color: Theme.line
  }
}
