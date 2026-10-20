pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Dialogs
import Eco.Core

// ColorField chooses an opaque color visually or commits a hexadecimal value.
Column {
  id: field
  property string value: Theme.dark.background
  readonly property alias input: box
  signal picked(string color)
  spacing: 4
  width: 210

  function act() { box.act() }
  function commit(text) {
    const color = text.trim().toLowerCase()
    if (Theme.rgb(color) !== null && color !== value.toLowerCase())
      picked(color)
  }

  Row {
    spacing: 8
    height: 30
    TraceButton {
      width: 30
      height: 30
      dense: true
      filled: true
      role: "action"
      tone: field.value
      tip: I18n.t("appearance.choose_color")
      Rectangle {
        anchors.fill: parent
        color: "transparent"
        border.color: Qt.alpha(Theme.foreground, 0.35)
      }
      onClicked: {
        dialog.selectedColor = field.value
        dialog.open()
      }
    }
    SyncedBox {
      id: box
      width: Math.max(80, field.width - 38)
      height: 30
      dense: true
      inline: true
      value: field.value
      placeholderText: I18n.t("appearance.hex")
      invalid: Theme.rgb(text.trim()) === null
      onEditingFinished: field.commit(text)
    }
  }
  FieldError {
    width: field.width
    text: box.invalid ? I18n.t("appearance.invalid_color") : ""
  }
  ColorDialog {
    id: dialog
    title: I18n.t("appearance.choose_color")
    onAccepted: field.commit(String(selectedColor))
  }
}
