pragma ComponentBehavior: Bound
import QtQuick

// ContextChips turns context slots on and off: one chip per configured slot,
// lit while on; `changed` gives the slots on after a click.
Flow {
  id: chips

  property var on: []
  signal changed(var slots)

  spacing: 6

  Repeater {
    model: Eco.contexts
    delegate: Chip {
      required property var modelData
      text: modelData.name
      checked: chips.on.includes(modelData.name)
      dim: !checked
      onClicked: chips.changed(checked ? chips.on.filter(name => name !== modelData.name) : chips.on.concat([modelData.name]))
    }
  }
}
