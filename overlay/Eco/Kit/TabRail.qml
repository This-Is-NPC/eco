pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// TabRail is an app's pages down the left edge, always in the same order; the
// one on screen is lit. Folded, it takes only the width of the pages' numbers;
// under the pointer it unfolds over what is beside it, each page naming itself
// and decoding. Each page is a stop of the Tab chain: ↑ ↓ Home End move the
// keyboard along the rail, Enter or Space chooses.
Item {
  id: rail

  // [{name, marked}], in order; a marked page, as one with changes not saved,
  // shows a dot in its corner, folded or not.
  required property var tabs
  property int current: 0
  signal chosen(int index)

  readonly property real tabHeight: 36
  readonly property real gap: 8
  // The width it keeps folded, which what is beside it leaves room for.
  readonly property real foldedWidth: 52
  // Unfolded while the pointer is over it or the keyboard is in it, and a
  // moment after; a tap on the lit tab unfolds or folds it, for touch.
  property bool unfolded: false
  property bool pinned: false
  readonly property bool wanted: hover.hovered || focusedTabs > 0 || pinned
  // How many of its tabs hold the keyboard: one, or none.
  property int focusedTabs: 0
  onWantedChanged: {
    if (wanted) {
      fold.stop()
      unfolded = true
    } else {
      fold.restart()
    }
  }
  HoverHandler { id: hover }
  Timer { id: fold; interval: 260; onTriggered: rail.unfolded = false }

  width: unfolded ? 260 : foldedWidth
  height: tabs.length * (tabHeight + gap) - gap

  // Gives the keyboard to the tab at a place on the rail.
  function reach(place) {
    tabRepeater.itemAt(Math.max(0, Math.min(tabs.length - 1, place))).forceActiveFocus(Qt.TabFocusReason)
  }

  Repeater {
    id: tabRepeater
    model: rail.tabs
    delegate: Rectangle {
      id: tab
      required property var modelData
      required property int index
      readonly property bool lit: index === rail.current
      readonly property color tone: lit ? Theme.primary : pointer.containsMouse ? Theme.foreground : Theme.dim

      y: index * (rail.tabHeight + rail.gap)
      width: rail.unfolded ? 220 : rail.foldedWidth
      height: rail.tabHeight
      Behavior on width { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }

      color: lit ? Qt.tint(Theme.background, Qt.alpha(Theme.primary, 0.08)) : Theme.background
      border.color: lit ? Qt.alpha(Theme.primary, 0.7) : pointer.containsMouse ? Qt.alpha(Theme.foreground, 0.35) : Theme.line

      // The keyboard reaches each page; Enter or Space chooses it.
      activeFocusOnTab: true
      onActiveFocusChanged: rail.focusedTabs += activeFocus ? 1 : -1
      Keys.onReturnPressed: rail.chosen(tab.index)
      Keys.onEnterPressed: rail.chosen(tab.index)
      Keys.onSpacePressed: rail.chosen(tab.index)
      Keys.onUpPressed: rail.reach(tab.index - 1)
      Keys.onDownPressed: rail.reach(tab.index + 1)
      Keys.onPressed: event => {
        if (event.key === Qt.Key_Home)
          rail.reach(0)
        else if (event.key === Qt.Key_End)
          rail.reach(rail.tabs.length - 1)
        else
          return
        event.accepted = true
      }
      FocusRing {}

      // Unfolding, each name decodes.
      Connections {
        target: rail
        function onUnfoldedChanged() { if (rail.unfolded) title.replay() }
      }

      // The names fold inside the hairline, which is never its own clip's edge.
      Item {
        anchors { fill: parent; margins: tab.border.width }
        clip: true

        Row {
          anchors { left: parent.left; verticalCenter: parent.verticalCenter; leftMargin: rail.unfolded ? 12 : (rail.foldedWidth - digit.implicitWidth) / 2 }
          Behavior on anchors.leftMargin { NumberAnimation { duration: 240; easing.type: Easing.OutCubic } }
          spacing: 10
          Label {
            id: digit
            anchors.verticalCenter: parent.verticalCenter
            text: String(tab.index + 1).padStart(2, "0")
            color: tab.lit ? Theme.primary : rail.unfolded ? Theme.line : tab.tone
            font.pixelSize: 10
            font.letterSpacing: 2
          }
          DecodeLabel {
            id: title
            anchors.verticalCenter: parent.verticalCenter
            opacity: rail.unfolded ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 200 } }
            value: tab.modelData.name.toUpperCase()
            color: tab.tone
            font.pixelSize: 10
            font.letterSpacing: 3
          }
        }

        Dot {
          visible: !!tab.modelData.marked
          anchors { right: parent.right; top: parent.top; margins: 6 }
          tone: Theme.warning
        }
      }

      MouseArea {
        id: pointer
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        // A tap on the lit tab of a folded rail unfolds it rather than choosing.
        onClicked: {
          if (tab.lit && !rail.unfolded) {
            rail.pinned = true
            return
          }
          rail.pinned = false
          rail.chosen(tab.index)
        }
      }
    }
  }
}
