pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// ContextSettings lists the context slots as cards: closed, a slot's name and
// how many files it has; open, its name, its files and the session kinds that
// start with it on. A new slot opens ready to fill in.
ColumnLayout {
  id: editor
  required property var slots
  // The session kinds a slot can start on with.
  required property var kinds
  // Messages by field path ("contexts.<index>"), as the settings window shows them.
  property var errors: ({})
  signal changed(var slots)
  // The index of the slot shown open, or -1.
  property int open: -1
  spacing: 8

  // Opens the card of the slot at `path` and gives its name the keyboard.
  function reveal(path) {
    const [list, at] = path.split(".")
    if (list !== "contexts")
      return
    editor.open = Number(at)
    Qt.callLater(() => cards.itemAt(Number(at)).nameField.input.forceActiveFocus())
  }

  Kicker { Layout.fillWidth: true; text: I18n.t("contexts.slots") }

  Repeater {
    id: cards
    model: editor.slots.length
    delegate: SettingsCard {
      id: card
      required property int index
      readonly property var slot: editor.slots[index]
      readonly property alias nameField: nameField

      function change(key, value) {
        const next = JSON.parse(JSON.stringify(editor.slots))
        next[index][key] = value
        editor.changed(next)
      }

      title: slot.name
      placeholder: I18n.t("contexts.number", { number: index + 1 })
      note: I18n.t("contexts.files_count", { n: slot.files.length })
      expanded: editor.open === index
      removeTip: I18n.t("contexts.remove")
      onToggled: editor.open = expanded ? -1 : index
      onRemoved: {
        const next = editor.slots.slice()
        next.splice(index, 1)
        editor.open = -1
        editor.changed(next)
      }

      Field {
        id: nameField
        Layout.fillWidth: true
        label: I18n.t("contexts.name")
        value: card.slot.name
        error: editor.errors["contexts." + card.index] || ""
        onEdited: text => card.change("name", text)
      }
      SettingsList {
        Layout.fillWidth: true
        items: card.slot.files
        label: I18n.t("contexts.files")
        placeholder: I18n.t("settings.file_placeholder")
        browse: true
        nameFilters: Eco.textFileFilters()
        onChanged: items => card.change("files", items)
      }
      ColumnLayout {
        Layout.fillWidth: true
        spacing: 6
        Caption { text: I18n.t("contexts.kinds") }
        Flow {
          Layout.fillWidth: true
          spacing: 6
          Repeater {
            model: editor.kinds
            delegate: Chip {
              required property string modelData
              readonly property bool on: card.slot.kinds.includes(modelData)
              text: Eco.kindName(modelData).toUpperCase()
              checked: on
              dim: !on
              onClicked: card.change("kinds", on ? card.slot.kinds.filter(kind => kind !== modelData) : card.slot.kinds.concat([modelData]))
            }
          }
        }
      }
    }
  }

  Chip {
    text: I18n.t("contexts.add")
    icon: "plus"
    onClicked: {
      editor.open = editor.slots.length
      editor.changed([...editor.slots, { name: "", files: [], kinds: [] }])
    }
  }
}
