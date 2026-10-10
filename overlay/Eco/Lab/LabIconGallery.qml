pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// Every icon of the set, read from Icon itself, so the gallery never falls behind.
Flow {
  id: gallery

  spacing: 8

  Icon { id: set; visible: false }

  Repeater {
    model: Object.keys(set.paths).sort()
    delegate: Rectangle {
      id: specimen
      required property string modelData
      width: 148
      height: 68
      color: Qt.alpha(Theme.primary, 0.025)
      border.color: Theme.line
      Icon {
        anchors { left: parent.left; leftMargin: 16; verticalCenter: parent.verticalCenter }
        name: specimen.modelData
        size: 22
        color: Theme.primary
      }
      Label {
        anchors { left: parent.left; leftMargin: 52; right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
        text: specimen.modelData
        color: Theme.dim
        font.pixelSize: 10
        elide: Text.ElideRight
      }
    }
  }
}
