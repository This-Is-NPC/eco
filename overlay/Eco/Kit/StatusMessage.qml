import QtQuick
import Eco.Core

// StatusMessage tells what happened or what is going on: a compact line, or a
// toast whose message wraps. Controls given to it sit on its right.
Rectangle {
  id: status

  default property alias actions: actionRow.data
  property string variant: "compact"
  property string message
  property string kind: "success"
  readonly property color tone: kind === "error" ? Theme.error : kind === "info" ? Theme.primary : Theme.success

  implicitHeight: variant === "toast" ? Math.max(68, toast.implicitHeight + 28) : 42
  color: variant === "toast" ? Qt.alpha(tone, 0.06) : Qt.alpha(Theme.foreground, 0.015)
  border.color: variant === "toast" ? Qt.alpha(tone, 0.5) : Theme.line

  HoverHandler { id: hover }
  Hint { visible: hover.hovered && status.width < 160; text: status.message }

  Row {
    visible: status.variant === "compact"
    anchors { left: parent.left; leftMargin: 14; verticalCenter: parent.verticalCenter }
    spacing: 10
    Rectangle {
      anchors.verticalCenter: parent.verticalCenter
      width: 6
      height: 6
      color: status.tone
    }
    Label {
      id: kindLabel
      visible: status.width >= 90
      text: I18n.t("status.kind." + status.kind)
      color: status.tone
      font.pixelSize: 9
      font.letterSpacing: 1.5
    }
    Label {
      visible: status.width >= 160
      width: Math.max(0, status.width - 48 - kindLabel.width - actionRow.room)
      text: status.message
      color: Theme.foreground
      font.pixelSize: 11
      elide: Text.ElideRight
    }
  }

  Column {
    id: toast
    visible: status.variant === "toast"
    anchors { left: parent.left; right: parent.right; leftMargin: 14; rightMargin: 14 + actionRow.room; verticalCenter: parent.verticalCenter }
    spacing: 7
    Label {
      text: I18n.t("status.kind." + status.kind)
      color: status.tone
      font.pixelSize: 9
      font.letterSpacing: 2
    }
    Label {
      width: parent.width
      text: status.message
      color: Theme.foreground
      font.pixelSize: 11
      wrapMode: Text.Wrap
    }
  }

  Row {
    id: actionRow
    // The width the controls take from the message.
    readonly property real room: visibleChildren.length > 0 ? width + 10 : 0
    anchors { right: parent.right; rightMargin: 12; verticalCenter: parent.verticalCenter }
    spacing: 8
  }
}
