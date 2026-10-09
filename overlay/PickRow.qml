import QtQuick
import QtQuick.Layouts

// PickRow names a choice with a Caption and picks it with the Dropdown under it,
// as a Field names its input. A `dim` pick, one that only inherits, reads
// dimmer; an `invalid` one, in the error colour.
ColumnLayout {
  id: row

  property string text
  property alias options: pick.options
  property alias current: pick.current
  property alias describe: pick.describe
  property alias dim: pick.dim
  property alias invalid: pick.invalid
  property alias label: pick.label
  signal picked(string option)
  spacing: 6

  Caption { text: row.text }
  Dropdown {
    id: pick
    onPicked: option => row.picked(option)
  }
}
