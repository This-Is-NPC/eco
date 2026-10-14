import QtQuick
import Eco.Core

// NoticeChip is a small notice the user may follow or close: a lit chip that
// opens what it tells about, and a × that puts it away. It fades in when it
// has something to tell and out when it is closed.
Row {
  id: notice

  property string text
  property string tip
  property string closeTip
  // Whether it has something to tell; without, it fades and takes no room.
  property bool shown: false
  // The text shown, kept while the notice fades.
  property string shownText: ""
  onTextChanged: if (text !== "") shownText = text
  Component.onCompleted: shownText = text
  signal opened()
  signal dismissed()

  spacing: 4
  visible: opacity > 0
  opacity: shown ? 1 : 0
  Behavior on opacity { NumberAnimation { duration: notice.shown ? 180 : 120; easing.type: Easing.OutCubic } }

  Chip {
    checked: true
    icon: "info"
    text: notice.shownText.toUpperCase()
    tip: notice.tip
    onClicked: notice.opened()
  }
  IconButton {
    implicitWidth: 26
    implicitHeight: 26
    name: "close"
    size: 10
    tip: notice.closeTip
    onClicked: notice.dismissed()
  }
}
