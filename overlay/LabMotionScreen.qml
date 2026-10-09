import QtQuick

Item {
  id: screen

  property bool shown: false
  property string index
  property string heading
  property string firstItem
  property string secondItem
  property string nextLabel
  signal navigate()

  Stage {
    anchors.fill: parent
    shown: screen.shown

    Rectangle {
      anchors.fill: parent
      color: Qt.alpha(Theme.foreground, 0.015)
      border.color: Theme.line

      Column {
        anchors { fill: parent; margins: 16 }
        spacing: 14
        Row {
          spacing: 10
          Label { text: screen.index; color: Theme.primary; font.pixelSize: 10; font.letterSpacing: 2 }
          DecodeLabel {
            value: screen.heading.toUpperCase()
            soft: true
            color: Theme.secondary
            font.pixelSize: 10
            font.letterSpacing: 2
          }
        }
        DecodeLabel {
          width: parent.width
          value: screen.firstItem
          soft: true
          color: Theme.foreground
          font.pixelSize: 12
          elide: Text.ElideRight
        }
        DecodeLabel {
          width: parent.width
          value: screen.secondItem
          soft: true
          color: Theme.dim
          font.pixelSize: 12
          elide: Text.ElideRight
        }
        TraceButton {
          role: "navigation"
          dense: true
          icon: "chevron-right"
          text: screen.nextLabel
          onClicked: screen.navigate()
        }
      }
    }
  }
}
