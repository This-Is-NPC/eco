pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import QtQuick.Layouts

// PageBar is the command bar over a page: the commands of the page on screen,
// which slide in when the page changes, ranked. BACK stands first, apart; then
// the page's main action, solid, on the left; across a gap, on the right, the
// others, outlined, the guarded ones last (CommandRow). When the names do not
// fit, each action but the main one shows only its icon, or its short name
// when it has none, and BACK only its chevron; on a bar narrower still, the
// main one too, and narrower again one menu button holds them all, BACK first.
Rectangle {
  id: bar

  // [{text, short, icon, tip, role, tone, group, checked, enabled, run,
  // shown}], in order; role is "back", "primary", "guarded" or left out.
  property var commands: []
  // Changes when the page does, so the commands arrive again.
  property int section: 0

  readonly property var shown: commands.filter(command => command.shown !== false)
  readonly property var back: shown.find(command => command.role === "back") || null
  readonly property var actions: shown.filter(command => command.role !== "back")
  // The least gap between the main action and the others.
  readonly property real gap: 24
  // The room the actions have beside BACK as only its chevron and its rule.
  readonly property real room: width - 16 - (back !== null ? 32 + 21 : 0) - gap

  color: Qt.alpha(Theme.background, 0.9)
  border.color: Theme.line
  clip: true

  onSectionChanged: arrive.restart()

  // BACK as named, to know the room it takes.
  TraceButton {
    id: backMeasure
    opacity: 0
    enabled: false
    dense: true
    icon: "chevron-left"
    text: bar.back ? bar.back.text : ""
  }

  RowLayout {
    anchors { fill: parent; leftMargin: 8; rightMargin: 8 }
    spacing: 10

    TraceButton {
      Layout.alignment: Qt.AlignVCenter
      Layout.preferredWidth: contextual.tight ? 32 : implicitWidth
      visible: bar.back !== null && !contextual.folded
      dense: true
      role: "navigation"
      icon: "chevron-left"
      text: bar.back && !contextual.tight ? bar.back.text : ""
      tip: bar.back ? bar.back.tip || bar.back.text.toLowerCase() : ""
      tone: Theme.dim
      onClicked: bar.back.run()
    }
    Rectangle {
      visible: bar.back !== null && bar.actions.length > 0 && !contextual.folded
      Layout.alignment: Qt.AlignVCenter
      implicitWidth: 1
      implicitHeight: 20
      color: Theme.line
    }

    CommandRow {
      id: contextual
      Layout.alignment: Qt.AlignVCenter
      Layout.fillWidth: true
      spread: true
      commands: bar.actions
      // Named while the names leave BACK its name.
      room: bar.room - (bar.back !== null ? backMeasure.implicitWidth - 32 : 0)
      fit: bar.room
      iconsWhenTight: true
      folds: bar.back !== null ? [Object.assign({ icon: "chevron-left" }, bar.back)] : []
      transform: Translate { id: shift }
    }

    ParallelAnimation {
      id: arrive
      NumberAnimation { target: contextual; property: "opacity"; from: 0; to: 1; duration: 200; easing.type: Easing.OutCubic }
      NumberAnimation { target: shift; property: "x"; from: 16; to: 0; duration: 260; easing.type: Easing.OutCubic }
    }
  }
}
