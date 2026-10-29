pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import QtQuick.Layouts

// PageBar is the command bar over a page: the commands of the page on screen,
// which slide in when the page changes, ranked. BACK stands first, apart; then
// the page's main action, solid, on the left; across a gap, on the right, the
// others, outlined, the guarded ones last (CommandRow). When the names do not
// fit, BACK shows only its chevron and the actions their short names, or,
// those too wide, each but the main one only its icon, or its short name when
// it has none; on a bar narrower still, the
// main one too, and narrower again one menu button holds them all, BACK first;
// and at its end the page's readout, which gives way first.
Rectangle {
  id: bar

  // [{text, short, icon, tip, role, tone, group, checked, enabled, run,
  // shown}], in order; role is "back", "primary", "guarded" or left out.
  property var commands: []
  // Changes when the page does, so the commands arrive again.
  property int section: 0
  // What the page counts, as "7 SESSIONS", at the bar's end; "" for none.
  property string readout: ""

  readonly property var shown: commands.filter(command => command.shown !== false)
  readonly property var back: shown.find(command => command.role === "back") || null
  readonly property var actions: shown.filter(command => command.role !== "back")
  // The least gap between the main action and the others.
  readonly property real gap: 24
  // The room the actions have beside BACK as only its chevron and its rule.
  readonly property real room: width - 16 - (back !== null ? 32 + 21 : 0) - gap
  // The room the readout takes with its rule.
  readonly property real readoutRoom: readoutMeasure.implicitWidth + 21
  // Shown while BACK as its chevron, the main action named and the others as
  // icons still fit beside it.
  readonly property bool readoutShown: readout !== ""
    && contextual.iconsWidth(true) + (back !== null ? 32 + 21 : 0) + gap + 16 + readoutRoom <= width

  color: Qt.alpha(Theme.background, 0.9)
  border.color: Theme.line

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
  // The readout, to know the room it takes.
  Label {
    id: readoutMeasure
    opacity: 0
    text: bar.readout
    font.pixelSize: 10
    font.letterSpacing: 3
  }

  // The commands slide in inside the hairline, which is never its own clip's edge.
  Item {
    anchors { fill: parent; margins: bar.border.width }
    clip: true

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
        room: bar.room - (bar.back !== null ? backMeasure.implicitWidth - 32 : 0) - (bar.readoutShown ? bar.readoutRoom : 0)
        fit: bar.room - (bar.readoutShown ? bar.readoutRoom : 0)
        iconsWhenTight: true
        folds: bar.back !== null ? [Object.assign({ icon: "chevron-left" }, bar.back)] : []
        transform: Translate { id: shift }
      }

      Rectangle {
        visible: bar.readoutShown
        Layout.alignment: Qt.AlignVCenter
        implicitWidth: 1
        implicitHeight: 20
        color: Theme.line
      }
      Label {
        visible: bar.readoutShown
        Layout.alignment: Qt.AlignVCenter
        Layout.rightMargin: 4
        text: bar.readout
        color: Theme.dim
        font.pixelSize: 10
        font.letterSpacing: 3
      }

      ParallelAnimation {
        id: arrive
        NumberAnimation { target: contextual; property: "opacity"; from: 0; to: 1; duration: 200; easing.type: Easing.OutCubic }
        NumberAnimation { target: shift; property: "x"; from: 16; to: 0; duration: 260; easing.type: Easing.OutCubic }
      }
    }
  }
}
