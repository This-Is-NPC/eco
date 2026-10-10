pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core

// ConfirmDialog asks before something that cannot be undone: a question, an
// optional warning, what else it offers on the left, a way back and the guarded
// action, already armed. The keyboard starts on the way back; Tab reaches the
// action.
ModalDialog {
  id: dialog

  default property alias leading: lead.data
  property string title
  property string question
  property string warning
  property string confirmText
  property string confirmIcon: "close"
  property string cancelText: I18n.t("dialog.cancel")
  signal confirmed()
  // The way back was pressed; Esc and a press outside only close it.
  signal cancelled()

  maxWidth: 460
  edge: Theme.error
  enter: null
  exit: null
  onOpened: cancel.forceActiveFocus()

  contentItem: DialogPanel {
    dialogOpen: dialog.visible
    revealItems: [questionLabel, warningLabel]
    implicitHeight: form.implicitHeight + 56
    index: "!"
    title: dialog.title
    active: true

    ColumnLayout {
      id: form
      anchors.fill: parent
      spacing: 18

      DecodeLabel {
        id: questionLabel
        Layout.fillWidth: true
        value: dialog.question
        soft: true
        wrapMode: Text.WordWrap
      }
      DecodeLabel {
        id: warningLabel
        Layout.fillWidth: true
        visible: dialog.warning !== ""
        value: dialog.warning
        soft: true
        color: Theme.error
        wrapMode: Text.WordWrap
      }
      RowLayout {
        Layout.fillWidth: true
        RowLayout { id: lead; spacing: 8 }
        Item { Layout.fillWidth: true }
        Chip {
          id: cancel
          text: dialog.cancelText
          onClicked: {
            dialog.close()
            dialog.cancelled()
          }
        }
        TraceButton {
          dense: true
          role: "guarded"
          icon: dialog.confirmIcon
          text: dialog.confirmText
          armed: true
          onClicked: {
            dialog.close()
            dialog.confirmed()
          }
        }
      }
    }
  }
}
