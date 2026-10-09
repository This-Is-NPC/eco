import QtQuick
import QtQuick.Controls.Basic as C

// PopupFrame is every popup a button opens below itself: a hairline frame that
// fades and settles in, kept this far inside the window near any edge. It takes
// the keyboard while open — Esc closes it — and gives it back to its opener. It
// closes once its button leaves — another page or view, or disabled — or the
// window goes inactive, so it never picks where it is not seen.
C.Popup {
  id: popup
  focus: true
  FocusKeeper { popup: popup }
  readonly property bool held: parent !== null && parent.visible && parent.enabled && parent.Window.active
  onHeldChanged: if (!held) close()
  y: parent.height + 4
  margins: 8
  background: Rectangle { color: Theme.background; border.color: Theme.line }
  enter: Transition {
    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 110 }
    NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: 110; easing.type: Easing.OutCubic }
  }
  exit: Transition { NumberAnimation { property: "opacity"; to: 0; duration: 90 } }
}
