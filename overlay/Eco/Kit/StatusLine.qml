import QtQuick
import Eco.Core

// StatusLine is the readout: the last message, and a warning while the
// daemon is unreachable. Information fades after a few seconds; errors stay
// until replaced.
Row {
  id: line
  property string message
  property bool error: false
  // Whether to warn that the daemon is unreachable.
  property bool offline: !Eco.connected
  // The message shortens before anything around it does.
  property real maxWidth: 380
  // The height of the message's box, as the line it sits on allows.
  property real boxHeight: 42
  spacing: 10
  // The least room a message shows in; with less it takes none.
  property real minWidth: 0
  // Without a message or a warning it takes no room, not even a layout's
  // spacing; nor does a message with less than `minWidth` to say it in.
  readonly property bool telling: message.length > 0 && maxWidth >= minWidth
  visible: (telling && session.opacity > 0) || offline

  // A new notice shows and starts fading; a language switch only rewrites it.
  property int count: Eco.noticeCount
  onCountChanged: if (message.length > 0) { session.opacity = 1; fade.restart() }
  Timer { id: fade; interval: 6000; onTriggered: if (!line.error) session.opacity = 0 }

  StatusMessage {
    id: session
    width: line.maxWidth
    height: line.boxHeight
    visible: line.telling
    Behavior on opacity { NumberAnimation { duration: 400 } }
    message: line.message
    kind: line.error ? "error" : "info"
  }
  Row {
    visible: line.offline
    spacing: 6
    Icon { anchors.verticalCenter: parent.verticalCenter; name: "warning"; size: 11; color: Theme.warning }
    Label { text: I18n.t("status.disconnected"); color: Theme.warning; font.pixelSize: 10; font.letterSpacing: 3 }
  }
}
