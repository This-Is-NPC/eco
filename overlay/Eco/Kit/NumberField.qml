import QtQuick

// NumberField is a Field for a whole number of at least `minimum`: it takes
// digits only, `numberEdited` carries each valid one, and leaving the input shows
// the last valid one again. `inline`, it is a value in a row, with no label:
// `act()` starts editing it, and `numberEdited` carries the number only once,
// when Enter or leaving commits one that differs; Esc puts it back.
Field {
  id: field
  property int number
  property int minimum: 1
  property bool inline: false
  signal numberEdited(int number)
  function act() { input.act() }
  value: String(number)
  input.inline: inline
  input.validator: RegularExpressionValidator { regularExpression: /[0-9]{0,9}/ }
  input.inputMethodHints: Qt.ImhDigitsOnly
  onEdited: text => {
    if (!field.inline && field.valid(text))
      field.numberEdited(Number(text))
  }
  function valid(text) { return text !== "" && Number(text) >= minimum }
  Connections {
    target: field.input
    function onEditingFinished() {
      const text = field.input.text
      if (field.inline && field.valid(text) && Number(text) !== field.number)
        field.numberEdited(Number(text))
      field.input.text = field.value
    }
  }
}
