import QtQuick

// SyncedBox is a TextBox that follows `value` without a binding, so typing
// never detaches it: a preset or a discard still updates an edited field.
TextBox {
  id: box
  property string value
  text: value
  onValueChanged: if (text !== value) text = value
}
