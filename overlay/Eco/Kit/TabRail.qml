pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// TabRail is an app's pages as a carousel down the left edge: the one on
// screen sits at the top beside the command bar, the others follow in order
// below it, the ones before it coming round after the last. Folded, it takes
// only the width of the pages' numbers; under the pointer it unfolds over
// what is beside it, each page naming itself, the one on screen stretched
// and decoding. Choosing one turns the carousel until it is at the top.
// Each page is a stop of the Tab chain: ↑ ↓ Home End move the keyboard along
// the carousel, Enter or Space chooses.
Item {
  id: rail

  // [{name, marked}], in order; a marked page, as one with changes not saved,
  // shows a dot in its corner, folded or not.
  required property var tabs
  property int current: 0
  // The height of the stretched tab, which the command bar shares.
  property real headHeight: 44

  signal chosen(int index)

  readonly property real tabHeight: 34
  readonly property real gap: 10
  // The width it keeps folded, which what is beside it leaves room for.
  readonly property real foldedWidth: 52
  // Unfolded while the pointer is over it or the keyboard is in it, and a
  // moment after; a tap on its head unfolds or folds it, for touch.
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
  height: headHeight + gap + (tabs.length - 1) * (tabHeight + gap)

  // Gives the keyboard to the tab at a place in the carousel.
  function reach(slot) {
    const at = Math.max(0, Math.min(tabs.length - 1, slot))
    tabRepeater.itemAt((at + current) % tabs.length).forceActiveFocus(Qt.TabFocusReason)
  }

  Repeater {
    id: tabRepeater
    model: rail.tabs
    delegate: Rectangle {
      id: tab
      required property var modelData
      required property int index
      // Its place in the carousel: 0 at the top.
      readonly property int slot: (index - rail.current + rail.tabs.length) % rail.tabs.length
      readonly property bool head: slot === 0
      readonly property color tone: head ? Theme.primary : pointer.containsMouse ? Theme.foreground : Theme.dim

      y: head ? 0 : rail.headHeight + rail.gap + (slot - 1) * (rail.tabHeight + rail.gap)
      width: !rail.unfolded ? rail.foldedWidth : head ? 260 : 200
      height: head ? rail.headHeight : rail.tabHeight
      Behavior on y { NumberAnimation { duration: 360; easing.type: Easing.OutCubic } }
      Behavior on width { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }
      Behavior on height { NumberAnimation { duration: 300; easing.type: Easing.OutCubic } }

      color: head ? Qt.tint(Theme.background, Qt.alpha(Theme.primary, 0.08)) : Theme.background
      border.color: head ? Qt.alpha(Theme.primary, 0.7) : pointer.containsMouse ? Qt.alpha(Theme.foreground, 0.35) : Theme.line
      // Further down, the carousel's names fade.
      readonly property real fade: head ? 1 : Math.max(0.4, 1 - (slot - 1) * 0.09)

      onHeadChanged: if (head) title.replay()

      // The keyboard reaches each page; Enter or Space chooses it.
      activeFocusOnTab: true
      onActiveFocusChanged: rail.focusedTabs += activeFocus ? 1 : -1
      Keys.onReturnPressed: rail.chosen(tab.index)
      Keys.onEnterPressed: rail.chosen(tab.index)
      Keys.onSpacePressed: rail.chosen(tab.index)
      Keys.onUpPressed: rail.reach(tab.slot - 1)
      Keys.onDownPressed: rail.reach(tab.slot + 1)
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
          anchors { left: parent.left; verticalCenter: parent.verticalCenter; leftMargin: rail.unfolded ? (tab.head ? 16 : 12) : (rail.foldedWidth - digit.implicitWidth) / 2 }
          Behavior on anchors.leftMargin { NumberAnimation { duration: 240; easing.type: Easing.OutCubic } }
          spacing: 10
          opacity: tab.fade
          Behavior on opacity { NumberAnimation { duration: 300 } }
          Label {
            id: digit
            anchors.verticalCenter: parent.verticalCenter
            text: String(tab.index + 1).padStart(2, "0")
            color: tab.head ? Theme.primary : rail.unfolded ? Theme.line : tab.tone
            font.pixelSize: tab.head && !rail.unfolded ? 13 : 10
            font.letterSpacing: 2
          }
          DecodeLabel {
            id: title
            anchors.verticalCenter: parent.verticalCenter
            opacity: rail.unfolded ? 1 : 0
            Behavior on opacity { NumberAnimation { duration: 200 } }
            value: tab.modelData.name.toUpperCase()
            color: tab.tone
            font.pixelSize: tab.head ? 13 : 10
            font.letterSpacing: tab.head ? 4 : 3
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
        // A tap on the head of a folded rail unfolds it rather than choosing.
        onClicked: {
          if (tab.head && !rail.unfolded) {
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
