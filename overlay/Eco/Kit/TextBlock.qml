import QtQuick
import QtQuick.Controls.Basic as C
import Eco.Core

// TextBlock wraps text inside the same full outline. Enter accepts and
// Shift+Enter breaks the line; without `submits`, Enter breaks the line too.
// Tab and Shift+Tab leave it.
C.TextArea {
  id: block

  property bool submits: true
  signal accepted()

  font.family: Theme.fontFamily
  font.pixelSize: 12
  color: Theme.foreground
  placeholderTextColor: Theme.dim
  selectionColor: Theme.primary
  selectedTextColor: Theme.badgeForeground
  wrapMode: TextEdit.Wrap
  leftPadding: 14
  rightPadding: 14
  topPadding: 12
  bottomPadding: 12
  Keys.onReturnPressed: event => {
    if (!block.submits || event.modifiers & Qt.ShiftModifier)
      event.accepted = false
    else
      block.accepted()
  }
  // Tab moves on, as from a one-line field, instead of typing a tab.
  Keys.onTabPressed: nextItemInFocusChain(true).forceActiveFocus(Qt.TabFocusReason)
  Keys.onBacktabPressed: nextItemInFocusChain(false).forceActiveFocus(Qt.BacktabFocusReason)
  background: InputFrame {
    implicitHeight: 84
    focused: block.activeFocus
  }
}
