import QtQuick

Rectangle {
  id: row

  property string title
  default property alias controls: actions.data

  implicitHeight: Math.max(62, actions.childrenRect.height + 24)
  color: Qt.tint(Theme.background, Qt.alpha(Theme.foreground, 0.025))
  border.color: Theme.line

  Label {
    anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
    width: 142
    text: row.title.toUpperCase()
    color: Theme.dim
    font.pixelSize: 10
    font.letterSpacing: 1.5
    wrapMode: Text.WordWrap
  }

  Flow {
    id: actions
    anchors { left: parent.left; leftMargin: 170; right: parent.right; rightMargin: 14; top: parent.top; topMargin: 12 }
    height: childrenRect.height
    spacing: 10
  }
}
