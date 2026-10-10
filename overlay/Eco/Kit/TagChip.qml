import QtQuick
import Eco.Core

// TagChip is a tag's chip in a filter: a click picks it; a right click or the
// menu key opens a menu to rename or delete the tag, as F2 and Delete do.
Chip {
  id: chip

  property string tag
  signal renameRequested()
  signal deleteRequested()

  text: I18n.t("tags.label", { tag: chip.tag })
  tip: I18n.t("tags.chip_tip")

  Keys.onPressed: event => {
    if (event.key === Qt.Key_F2)
      chip.renameRequested()
    else if (event.key === Qt.Key_Delete)
      chip.deleteRequested()
    else if (event.key === Qt.Key_Menu)
      menu.open()
    else
      return
    event.accepted = true
  }

  MouseArea {
    anchors.fill: parent
    acceptedButtons: Qt.RightButton
    onClicked: menu.open()
  }

  MenuPopup {
    id: menu
    entries: [
      { icon: "edit", text: I18n.t("tags.rename") },
      { icon: "trash", text: I18n.t("tags.delete"), guarded: true }
    ]
    onPicked: index => index === 0 ? chip.renameRequested() : chip.deleteRequested()
  }
}
