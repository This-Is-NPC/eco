pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Dialogs
import QtQuick.Layouts
import Eco.Core

// SettingsList edits an ordered list, one compact row per item: its position,
// the item, a step up and a removal. Items are typed, or picked
// from `choices` when given, each shown through `describe`. With `browse`,
// files are also picked from a file dialog, filtered by `nameFilters`.
ColumnLayout {
  id: list
  required property var items
  // Titles the list with a kicker; empty when the panel around it already does.
  property string label
  property string placeholder
  // When set, items are picked from these instead of typed.
  property var choices: []
  property var describe: item => item
  property bool browse: false
  property var nameFilters: []
  // What is wrong with the list, shown under it.
  property string error
  signal changed(var items)

  readonly property bool picked: choices.length > 0

  function moved(from, to) {
    const next = list.items.slice()
    next.splice(to, 0, next.splice(from, 1)[0])
    return next
  }

  spacing: 6

  Kicker { Layout.fillWidth: true; visible: list.label !== ""; text: list.label }

  Repeater {
    model: list.items.length
    delegate: RowLayout {
      id: row
      required property int index
      readonly property bool first: index === 0
      Layout.fillWidth: true
      spacing: 6

      Label {
        Layout.preferredWidth: 22
        text: String(row.index + 1).padStart(2, "0")
        color: Theme.dim
        font.pixelSize: 10
        font.letterSpacing: 1
      }
      SyncedBox {
        visible: !list.picked
        Layout.fillWidth: true
        dense: true
        value: list.items[row.index]
        onTextEdited: {
          const next = list.items.slice()
          next[row.index] = text
          list.changed(next)
        }
      }
      TextBox {
        visible: list.picked
        Layout.fillWidth: true
        dense: true
        readOnly: true
        text: list.describe(list.items[row.index])
      }
      IconButton {
        enabled: !row.first
        opacity: enabled ? 1 : 0.3
        name: "chevron-up"
        tip: I18n.t("settings.move_up")
        onClicked: list.changed(list.moved(row.index, row.index - 1))
      }
      IconButton {
        name: "close"
        tone: Theme.error
        tip: I18n.t("settings.remove")
        onClicked: {
          const next = list.items.slice()
          next.splice(row.index, 1)
          list.changed(next)
        }
      }
    }
  }

  RowLayout {
    Layout.fillWidth: true
    visible: !list.picked
    spacing: 6
    TextBox {
      id: newItem
      Layout.fillWidth: true
      dense: true
      placeholderText: list.placeholder
      onAccepted: add.clicked()
    }
    IconButton {
      visible: list.browse
      name: "folder"
      tip: I18n.t("settings.browse")
      onClicked: picker.open()
    }
    IconButton {
      id: add
      name: "plus"
      tip: I18n.t("settings.add_item")
      onClicked: {
        const value = newItem.text.trim()
        if (!value)
          return
        list.changed([...list.items, value])
        newItem.text = ""
      }
    }
  }

  FileDialog {
    id: picker
    title: I18n.t("settings.browse")
    fileMode: FileDialog.OpenFiles
    nameFilters: list.nameFilters
    onAccepted: {
      const added = selectedFiles.map(url => Eco.localPath(url)).filter(path => !list.items.includes(path))
      if (added.length)
        list.changed([...list.items, ...added])
    }
  }

  Dropdown {
    visible: list.picked && options.length > 0
    options: list.choices.filter(choice => !list.items.includes(choice))
    current: ""
    label: I18n.t("settings.add_choice")
    describe: list.describe
    onPicked: choice => list.changed([...list.items, choice])
  }

  FieldError { Layout.fillWidth: true; text: list.error }
}
