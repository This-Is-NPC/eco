import QtQuick
import QtQuick.Controls.Basic as C

// ModalDialog is the frame every eco dialog shares: centred in the window
// wherever it is declared, at most `maxWidth` wide, over a dimmed window,
// fading and settling in. Its top edge stays where it opened while it grows or
// shrinks, unless it would leave the window. It takes the keyboard — Esc closes it — and gives it
// back to what had it.
C.Popup {
  id: modal
  property int maxWidth: 520
  property color edge: Theme.line

  // Where the top edge opened, or -1 before it opens.
  property real openedTop: -1

  parent: C.Overlay.overlay
  x: Math.round((parent.width - width) / 2)
  y: Math.round(Math.max(24, Math.min(openedTop < 0 ? (parent.height - height) / 2 : openedTop, parent.height - height - 24)))
  width: Math.min(parent.width - 48, maxWidth)
  onAboutToShow: openedTop = -1
  onOpened: openedTop = y
  modal: true
  focus: true
  FocusKeeper { popup: modal }
  padding: 0
  background: Rectangle { color: Theme.background; border.color: edge }
  C.Overlay.modal: Rectangle { color: Qt.alpha(Theme.background, 0.75) }
  enter: Transition {
    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 140 }
    NumberAnimation { property: "scale"; from: 0.97; to: 1; duration: 140; easing.type: Easing.OutCubic }
  }
  exit: Transition {
    NumberAnimation { property: "opacity"; to: 0; duration: 100 }
  }
}
