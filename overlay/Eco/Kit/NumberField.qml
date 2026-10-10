import QtQuick

// NumberField is a Field for a whole number of at least `minimum`: it takes
// digits only, `numberEdited` carries each valid one, and leaving the input shows
// the last valid one again.
Field {
  id: field
  property int number
  property int minimum: 1
  signal numberEdited(int number)
  value: String(number)
  input.validator: RegularExpressionValidator { regularExpression: /[0-9]{0,9}/ }
  input.inputMethodHints: Qt.ImhDigitsOnly
  onEdited: text => {
    if (text !== "" && Number(text) >= field.minimum)
      field.numberEdited(Number(text))
  }
  Connections {
    target: field.input
    function onEditingFinished() { field.input.text = field.value }
  }
}
