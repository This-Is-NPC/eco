import QtQuick
import Eco.Core

// Dropdown picks one option: a quiet button showing the current one, and a
// list below it, as wide as its longest option. `describe` turns an option
// into its label; a `dim` one, as when it only inherits its choice, reads dimmer,
// and an `invalid` one takes the error colour.
Item {
  id: dropdown

  property var options: []
  property string current
  property var describe: option => option
  property bool emphasized: false
  property bool dim: false
  property bool invalid: false
  // What the button shows; the current option's description when empty.
  property string label
  signal picked(string option)

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  Chip {
    id: button
    text: dropdown.label || dropdown.describe(dropdown.current)
    checked: list.opened || dropdown.emphasized || dropdown.invalid
    accent: dropdown.invalid ? Theme.error : Theme.primary
    dim: dropdown.dim
    onClicked: list.opened ? list.close() : list.open()
    trailing: 14
    Icon {
      anchors { right: parent.right; rightMargin: 8; verticalCenter: parent.verticalCenter }
      name: "chevron-down"
      size: 11
      color: button.ink
      rotation: list.opened ? 180 : 0
      Behavior on rotation { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
    }
  }

  MenuPopup {
    id: list
    minimumWidth: button.width
    entries: dropdown.options.map(option => ({ text: dropdown.describe(option), chosen: option === dropdown.current }))
    onPicked: index => {
      const option = dropdown.options[index]
      if (option !== dropdown.current)
        dropdown.picked(option)
    }
  }
}
