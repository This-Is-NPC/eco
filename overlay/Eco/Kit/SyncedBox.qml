import QtQuick

// SyncedBox is a TextBox that follows `value` without a binding, so typing
// never detaches it: a preset or a discard still updates an edited field.
// `inline`, it is a value in a row: `act()` starts editing it, all selected,
// and Esc puts `value` back before passing on, so leaving commits nothing.
TextBox {
  id: box
  property string value
  property bool inline: false
  text: value
  onValueChanged: if (text !== value) text = value

  function act() {
    forceActiveFocus()
    selectAll()
  }
  Keys.onEscapePressed: event => {
    if (box.inline)
      box.text = box.value
    event.accepted = false
  }
}
