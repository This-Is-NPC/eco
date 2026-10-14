pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// TabRail is an app's pages down the left edge, in their fixed order, the one
// on screen marked: tinted, outlined and barred on its left in the accent.
// Labelled, each shows its number and its name; folded, only its number, its
// name in the tip. It is one stop of the Tab chain: ↑ ↓ Home End move the
// keyboard along it, Enter or Space chooses.
Item {
  id: rail

  // [{name, marked}], in order; a marked page, as one with changes not saved,
  // shows a dot in its corner.
  required property var tabs
  property int current: 0
  property bool labelled: true

  signal chosen(int index)

  // Each tab's height; the gap between them narrows with it.
  property real tabHeight: 34
  readonly property real gap: tabHeight < 30 ? 4 : 6
  readonly property real foldedWidth: 52
  readonly property real labelledWidth: 188
  // The tab the keyboard is on while the rail holds it; the one on screen otherwise.
  property int cursor: current

  width: labelled ? labelledWidth : foldedWidth
  height: tabs.length * (tabHeight + gap) - gap

  activeFocusOnTab: true
  onActiveFocusChanged: if (!activeFocus) cursor = current
  onCurrentChanged: cursor = current

  function move(index) { cursor = Math.max(0, Math.min(tabs.length - 1, index)) }
  Keys.onUpPressed: move(cursor - 1)
  Keys.onDownPressed: move(cursor + 1)
  Keys.onReturnPressed: chosen(cursor)
  Keys.onEnterPressed: chosen(cursor)
  Keys.onSpacePressed: chosen(cursor)
  Keys.onPressed: event => {
    if (event.key === Qt.Key_Home)
      move(0)
    else if (event.key === Qt.Key_End)
      move(tabs.length - 1)
    else
      return
    event.accepted = true
  }

  Repeater {
    model: rail.tabs
    delegate: Rectangle {
      id: tab
      required property var modelData
      required property int index
      readonly property bool head: index === rail.current
      readonly property bool keyed: rail.activeFocus && index === rail.cursor
      readonly property color tone: head ? Theme.primary : pointer.containsMouse || keyed ? Theme.foreground : Theme.dim

      y: index * (rail.tabHeight + rail.gap)
      width: rail.width
      height: rail.tabHeight
      color: head ? Qt.tint(Theme.background, Qt.alpha(Theme.primary, 0.08)) : Theme.background
      border.color: head ? Qt.alpha(Theme.primary, 0.7) : pointer.containsMouse ? Qt.alpha(Theme.foreground, 0.35) : Theme.line
      Behavior on color { ColorAnimation { duration: 150 } }
      Behavior on border.color { ColorAnimation { duration: 150 } }

      onHeadChanged: if (head) title.replay()

      Rectangle {
        visible: tab.head
        width: 2
        height: parent.height
        color: Theme.primary
      }

      Row {
        anchors { left: parent.left; verticalCenter: parent.verticalCenter; leftMargin: rail.labelled ? 14 : (rail.foldedWidth - digit.implicitWidth) / 2 }
        spacing: 10
        Label {
          id: digit
          anchors.verticalCenter: parent.verticalCenter
          text: String(tab.index + 1).padStart(2, "0")
          color: tab.tone
          font.pixelSize: 10
          font.letterSpacing: 2
        }
        DecodeLabel {
          id: title
          anchors.verticalCenter: parent.verticalCenter
          visible: rail.labelled
          width: Math.min(implicitWidth, rail.width - 14 - digit.implicitWidth - 10 - 16)
          autoPlay: false
          value: tab.modelData.name.toUpperCase()
          color: tab.tone
          font.pixelSize: 10
          font.letterSpacing: 3
          elide: Text.ElideRight
        }
      }

      Dot {
        visible: !!tab.modelData.marked
        anchors { right: parent.right; top: parent.top; margins: 6 }
        tone: Theme.warning
      }

      MouseArea {
        id: pointer
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: rail.chosen(tab.index)
      }
      FocusRing { shown: tab.keyed }
      Hint { visible: !rail.labelled && (pointer.containsMouse || tab.keyed); text: tab.modelData.name }
    }
  }
}
