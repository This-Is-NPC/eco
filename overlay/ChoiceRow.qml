import QtQuick

// ChoiceRow is one option in a list of choices: its icon, or a dot when it is
// the chosen one; its text; a tint of the accent when chosen and a lighter row
// under the pointer, outlined when the keyboard is on it. A `guarded` option
// reads in the error colour.
Rectangle {
  id: row

  property string text
  property string icon
  property bool chosen: false
  property bool guarded: false
  // The row the keyboard is on.
  property bool current: false
  property int elide: Text.ElideRight
  readonly property color tone: guarded ? Theme.error : Theme.foreground
  signal picked()

  implicitHeight: 28
  readonly property bool lit: pick.containsMouse || current
  color: chosen ? Qt.alpha(Theme.primary, 0.12) : (lit ? Theme.highlight : "transparent")
  border.color: current ? Theme.primary : "transparent"
  Behavior on color { ColorAnimation { duration: 110 } }

  Dot {
    visible: row.chosen && row.icon === ""
    anchors { left: parent.left; leftMargin: 11; verticalCenter: parent.verticalCenter }
  }
  Icon {
    visible: row.icon !== ""
    anchors { left: parent.left; leftMargin: 11; verticalCenter: parent.verticalCenter }
    name: row.icon
    size: 12
    color: row.lit ? row.tone : Theme.dim
  }
  Label {
    anchors { left: parent.left; leftMargin: row.icon !== "" ? 32 : 24; right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
    text: row.text
    color: row.chosen ? Theme.primary : (row.lit ? row.tone : (row.guarded ? Qt.alpha(Theme.error, 0.8) : Theme.dim))
    font.pixelSize: 11
    font.letterSpacing: 1
    elide: row.elide
  }
  MouseArea {
    id: pick
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: row.picked()
  }
}
