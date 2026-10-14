pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// NewSourceDialog names a new audio source to hear through one device. A name
// another source has is refused where it is typed; ADD AUDIO SOURCE or Enter
// hands the name on, and the dialog closes.
ModalDialog {
  id: dialog

  // The device's id and what it is called, and the names the sources have.
  property string device
  property string deviceLabel
  property var names: []
  readonly property string name: box.text.trim()
  readonly property string problem: names.includes(name) ? I18n.t("settings.error_taken") : ""
  signal named(string device, string name)

  // Asks a name for a source heard through the device `id`, called `label`.
  function ask(id, label) {
    device = id
    deviceLabel = label
    box.text = ""
    open()
  }
  function accept() {
    if (name === "" || problem !== "")
      return
    close()
    named(device, name)
  }

  onOpened: box.forceActiveFocus()

  contentItem: DialogPanel {
    dialogOpen: dialog.visible
    implicitHeight: form.implicitHeight + 56
    title: I18n.t("audio.new_source_title")
    active: true

    ColumnLayout {
      id: form
      anchors.fill: parent
      spacing: 18

      Label {
        Layout.fillWidth: true
        text: I18n.t("audio.new_source_note", { device: dialog.deviceLabel })
        color: Theme.dim
        font.pixelSize: 11
        wrapMode: Text.Wrap
      }
      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: I18n.t("settings.column_name").toUpperCase() }
        TextBox {
          id: box
          Layout.fillWidth: true
          dense: true
          placeholderText: I18n.t("audio.new_placeholder")
          invalid: dialog.problem !== ""
          onAccepted: dialog.accept()
        }
        FieldError { Layout.fillWidth: true; text: dialog.problem }
      }
      DialogFooter {
        primaryIcon: "plus"
        primaryText: I18n.t("audio.add_source")
        primaryEnabled: dialog.name !== "" && dialog.problem === ""
        onCancelled: dialog.close()
        onAccepted: dialog.accept()
      }
    }
  }
}
