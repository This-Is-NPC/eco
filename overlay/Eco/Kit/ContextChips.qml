pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// ContextChips turns context slots on and off: one chip per configured slot,
// lit while on; `changed` gives the slots on after a click. With a `reason`
// every chip is off and says why on hover.
Flow {
  id: chips

  property var on: []
  // Why the slots cannot be turned now; empty while they can.
  property string reason
  signal changed(var slots)

  spacing: 6

  Repeater {
    model: Eco.contexts
    delegate: Chip {
      required property var modelData
      text: modelData.name
      checked: chips.on.includes(modelData.name)
      dim: !checked
      reason: chips.reason
      onClicked: chips.changed(checked ? chips.on.filter(name => name !== modelData.name) : chips.on.concat([modelData.name]))
    }
  }
}
