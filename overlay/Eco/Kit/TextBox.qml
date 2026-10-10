import QtQuick
import QtQuick.Controls.Basic as C
import Eco.Core

// TextBox is a one-line input with a full outline that takes the accent on focus;
// `dense` makes it a list row's height; `invalid` outlines it in the error colour.
C.TextField {
  id: box
  property bool dense: false
  property bool invalid: false
  font.family: Theme.fontFamily
  font.pixelSize: dense ? 12 : 13
  color: Theme.foreground
  placeholderTextColor: Theme.dim
  selectionColor: Theme.primary
  selectedTextColor: Theme.badgeForeground
  leftPadding: dense ? 10 : 14
  rightPadding: dense ? 10 : 14
  topPadding: 6
  bottomPadding: 6
  background: InputFrame {
    implicitHeight: box.dense ? 30 : 44
    focused: box.activeFocus
    invalid: box.invalid
  }
}
