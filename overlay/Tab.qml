import QtQuick

// Tab is a section switch: a two-digit index and a tracked name, underlined
// by a hairline in the accent while shown; a dot in the error colour after
// the name while something on its page needs fixing.
Item {
  id: tab

  required property string label
  required property string digit
  required property bool current
  property bool alert: false

  signal chosen()

  readonly property color tone: current ? Theme.primary : (mouse.containsMouse ? Theme.foreground : Theme.dim)

  width: body.width
  height: body.height + 8
  activeFocusOnTab: true
  transform: Translate { id: nudge }

  function choose() {
    if (!current)
      depart.restart()
    chosen()
  }
  Keys.onReturnPressed: choose()
  Keys.onSpacePressed: choose()

  Row {
    id: body
    spacing: 8
    Label { text: tab.digit; color: tab.current ? Theme.primary : Theme.line; font.pixelSize: 10; font.letterSpacing: 2 }
    Label { text: tab.label.toUpperCase(); color: tab.tone; font.pixelSize: 11; font.letterSpacing: 3 }
  }
  // In the gap after the name, so the tabs keep their places.
  Dot { visible: tab.alert; x: body.width + 4; y: (body.height - height) / 2; tone: Theme.error }
  Rectangle { anchors.bottom: parent.bottom; width: body.width; height: 1; color: tab.current ? Theme.primary : "transparent" }
  FocusRing {}

  // The tab chosen slides right with the page leaving to the left.
  SequentialAnimation {
    id: depart
    NumberAnimation { target: nudge; property: "x"; from: 0; to: 14; duration: 180; easing.type: Easing.OutCubic }
    NumberAnimation { target: nudge; property: "x"; to: 0; duration: 240; easing.type: Easing.OutCubic }
  }

  MouseArea {
    id: mouse
    anchors.fill: parent
    hoverEnabled: true
    cursorShape: Qt.PointingHandCursor
    onClicked: tab.choose()
  }
}
