import QtQuick
import QtQuick.Layouts
import Eco.Core

// DialogFooter is the last row of every dialog: what else it offers on the
// left, then the way back and its primary action on the right. Esc closes the
// dialog as the way back does; Enter in its fields runs the primary action.
RowLayout {
  id: footer

  default property alias leading: lead.data
  property string cancelText: I18n.t("dialog.cancel")
  // The primary action's text; none leaves only the way back.
  property string primaryText
  property string primaryIcon
  property bool primaryEnabled: true
  // Reads dimmer while it would only say what is missing.
  property bool primaryDim: false
  signal cancelled()
  signal accepted()

  Layout.fillWidth: true
  spacing: 8

  RowLayout { id: lead; spacing: 8 }
  Item { Layout.fillWidth: true }
  Chip {
    text: footer.cancelText
    onClicked: footer.cancelled()
  }
  Chip {
    visible: footer.primaryText !== ""
    icon: footer.primaryIcon
    text: footer.primaryText
    checked: true
    enabled: footer.primaryEnabled
    dim: footer.primaryDim
    onClicked: footer.accepted()
  }
}
