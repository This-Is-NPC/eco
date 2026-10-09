import QtQuick
import QtQuick.Layouts

// Field is a tracked label over a framed input; `edited` carries the new text
// as typed and `committed` once it is left or Enter is pressed. An `error`
// outlines the input and shows under it.
ColumnLayout {
  id: field
  property string label
  property string value
  property string placeholder
  property string error
  readonly property alias input: box
  signal edited(string text)
  signal committed(string text)
  spacing: 6
  Caption { text: field.label.toUpperCase() }
  SyncedBox {
    id: box
    Layout.fillWidth: true
    dense: true
    value: field.value
    placeholderText: field.placeholder
    invalid: field.error !== ""
    onTextEdited: field.edited(text)
    onEditingFinished: field.committed(text)
  }
  FieldError { Layout.fillWidth: true; text: field.error }
}
