pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// RenameDialog edits a session's title and kind — the open session or a stored
// one — or, given {personId, name}, a person's name in every session, or, given
// {tag}, a tag in every session; given {newPerson: true} it names a person to
// add. A name someone else already has is not saved on Enter: the dialog offers
// to merge the two into that person, or to save and keep both; a new person
// cannot take it. A tag renamed into another tag says the two become one.
ModalDialog {
  id: dialog

  property string sessionId
  property string personId
  // Whether the name is for a person to add.
  property bool adding: false
  // The tag renamed, or "".
  property string tagFrom
  property string kind
  // Someone else with the name typed for the person, or null.
  readonly property var namesake: personId === "" && !adding ? null : Eco.people.find(p =>
    p.id !== personId && p.name.toLowerCase() === titleBox.text.trim().toLowerCase()) || null
  // The other tag the one renamed would join, or "".
  readonly property string joined: tagFrom === "" ? "" : Eco.tagNames.find(tag =>
    !Eco.sameTag(tag, tagFrom) && Eco.sameTag(tag, titleBox.text.trim().replace(/\s+/g, " "))) || ""

  function edit(target) {
    adding = target.newPerson === true
    personId = target.personId || ""
    tagFrom = target.tag || ""
    sessionId = personId || tagFrom || adding ? "" : target.id
    kind = target.kind || ""
    titleBox.text = personId ? target.name : adding ? "" : tagFrom || target.title
    open()
  }

  function save() {
    if (adding) {
      if (titleBox.text.trim() === "")
        return
      Eco.addPerson(titleBox.text.trim())
    } else if (personId)
      Eco.renamePerson(personId, titleBox.text)
    else if (tagFrom)
      Eco.renameTag(tagFrom, titleBox.text)
    else
      Eco.renameSession(sessionId, titleBox.text, kind)
    close()
  }

  function merge() {
    Eco.mergePeople(namesake.id, personId)
    close()
  }

  onOpened: titleBox.forceActiveFocus()

  contentItem: DialogPanel {
    dialogOpen: dialog.visible
    implicitHeight: form.implicitHeight + 56
    title: I18n.t(dialog.adding ? "people.add_title" : dialog.personId ? "people.rename_title" : dialog.tagFrom ? "tags.rename_title" : "rename.title")
    active: true

    ColumnLayout {
      id: form
      anchors.fill: parent
      spacing: 18

      ColumnLayout {
        Layout.fillWidth: true
        spacing: 2
        Caption { text: dialog.personId || dialog.adding ? I18n.t("people.column.name").toUpperCase() : dialog.tagFrom ? I18n.t("tags.name") : I18n.t("dialog.name") }
        TextBox {
          id: titleBox
          Layout.fillWidth: true
          placeholderText: dialog.personId || dialog.tagFrom || dialog.adding ? "" : I18n.t("dialog.name_placeholder")
          onAccepted: if (!dialog.namesake) dialog.save()
        }
      }

      Label {
        Layout.fillWidth: true
        visible: dialog.namesake !== null
        text: dialog.namesake ? I18n.t(dialog.adding ? "people.add_taken" : "people.rename_taken", { name: dialog.namesake.name }) : ""
        color: Theme.warning
        font.pixelSize: 11
        wrapMode: Text.Wrap
      }

      Label {
        Layout.fillWidth: true
        visible: dialog.joined !== ""
        text: I18n.t("tags.rename_joins", { tag: dialog.joined })
        color: Theme.warning
        font.pixelSize: 11
        wrapMode: Text.Wrap
      }

      ColumnLayout {
        visible: !dialog.personId && !dialog.tagFrom && !dialog.adding
        spacing: 4
        Caption { text: I18n.t("dialog.kind") }
        Dropdown {
          // A kind no longer configured stays offered, so saving keeps it.
          options: Eco.kinds.includes(dialog.kind) ? Eco.kinds : Eco.kinds.concat([dialog.kind])
          current: dialog.kind
          describe: kind => Eco.kindName(kind)
          onPicked: kind => dialog.kind = kind
        }
      }

      // Someone else with the name: merging is the primary action, saving
      // and keeping both the other one. A person to add needs a name no one has.
      DialogFooter {
        primaryIcon: dialog.adding ? "plus" : dialog.namesake ? "merge" : "enter"
        primaryText: dialog.adding ? I18n.t("people.add") : dialog.namesake ? I18n.t("people.merge") : I18n.t("rename.save")
        primaryEnabled: !dialog.adding || (dialog.namesake === null && titleBox.text.trim() !== "")
        onCancelled: dialog.close()
        onAccepted: dialog.namesake && !dialog.adding ? dialog.merge() : dialog.save()
        Chip { visible: dialog.namesake !== null && !dialog.adding; text: I18n.t("rename.save"); onClicked: dialog.save() }
      }
    }
  }
}
