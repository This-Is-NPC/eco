pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import QtQuick.Layouts

// PageBar is the command bar beside a TabRail's head, which names the page:
// the commands of the page on screen, which slide in when the page changes,
// ranked, and at its end the app's readout. BACK stands first, apart; then
// the page's main action, solid; then the others, outlined, those of one
// `group` joined as one control whose `checked` one is lit; then, at the end,
// its tools (`place: "end"`), each only an icon with its name in the tip.
// When the names do not fit, each action but the main one shows only its
// icon, or its short name when it has none, and BACK only its chevron; on a
// bar narrower still, the main one too, and narrower again one menu button
// holds them all, BACK first.
Rectangle {
  id: bar

  // [{text, short, icon, tip, role, tone, group, checked, place, enabled,
  // run, shown}], in order; role is "back", "primary", "guarded" or left out.
  property var commands: []
  // Changes when the page does, so the commands arrive again.
  property int section: 0
  // The readout: the app's last message, whether it is an error, whether the
  // server is unreachable, and how many messages it has given.
  property string message: ""
  property bool error: false
  property bool offline: false
  property int notices: 0

  readonly property var shown: commands.filter(command => command.shown !== false)
  readonly property var back: shown.find(command => command.role === "back") || null
  readonly property var tools: shown.filter(command => command.place === "end")
  // The page's actions, in their order.
  readonly property var actions: shown.filter(command => command.role !== "back" && command.place !== "end")
  // The room the actions have beside the tools, and BACK, as only its chevron.
  readonly property real room: width - 40 - toolRow.width - (back !== null ? 53 : 0)

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
    anchors { fill: parent; leftMargin: 8; rightMargin: 12 }
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
      commands: bar.actions
      // Named while the names leave BACK its name and the readout its room.
      room: bar.room - (bar.back !== null ? backMeasure.implicitWidth - 32 : 0) - 200
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

    Item { Layout.fillWidth: true }

    StatusLine {
      Layout.alignment: Qt.AlignVCenter
      message: bar.message
      error: bar.error
      offline: bar.offline
      count: bar.notices
      maxWidth: Math.max(0, bar.width - contextual.width - toolRow.width - 140)
    }

    Row {
      id: toolRow
      Layout.alignment: Qt.AlignVCenter
      spacing: 6
      Repeater {
        model: bar.tools
        delegate: IconButton {
          required property var modelData
          name: modelData.icon || "more"
          tip: modelData.tip || modelData.text
          tone: modelData.tone || Theme.primary
          enabled: modelData.enabled !== false
          opacity: enabled ? 1 : 0.35
          onClicked: modelData.run()
        }
      }
    }
  }
}
