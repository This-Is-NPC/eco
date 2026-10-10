import QtQuick
import Eco.Core

// FieldError says what is wrong with the field above it; left out while empty.
Label {
  visible: text !== ""
  color: Theme.error
  font.pixelSize: 11
  wrapMode: Text.Wrap
}
