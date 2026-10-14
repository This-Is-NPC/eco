pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SlotView is one context slot: the files a session sends with its answers
// while the slot is on there, and the session kinds it starts on in, each a
// link. Edited: its name, its files and those kinds. A new slot starts empty.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  // Its place in the config being edited: past the end for a new one.
  readonly property int at: item.base === null ? -1 : fresh ? (item.base.contexts || []).length
    : (item.base.contexts || []).findIndex(slot => slot.name === section.slotName(key))
  readonly property var slot: (item.active ? item.draft.contexts[at] : (config.contexts || []).find(slot => slot.name === section.slotName(key)))
    || ({ name: "", files: [], kinds: [] })
  readonly property string path: "contexts." + at

  function change(key, value) {
    const at = page.at
    item.update(d => d.contexts[at][key] = value)
  }

  kind: I18n.t("answers.slot")
  title: slot.name
  placeholder: I18n.t("answers.slot_new")
  nextKey: section.slotId(slot.name.trim())
  draft: ItemDraft { id: item; paths: page.at >= 0 ? [page.path] : [] }
  Component.onCompleted: if (fresh) item.begin(d => d.contexts = (d.contexts || []).concat([{ name: "", files: [], kinds: [] }]))

  facts: [
    [I18n.t("contexts.files"), slot.files.join("\n")],
    [I18n.t("contexts.kinds"), section.kindNames(slot.kinds)]
  ]
  details: [ItemNote { text: I18n.t("answers.slot_note") }]
  related: [{ title: settings.sections[settings.at.sessions].name,
    items: slot.kinds.map(kind => settings.link(settings.at.sessions, kind, section.kindName(kind))) }]

  Field {
    Layout.fillWidth: true
    label: I18n.t("contexts.name")
    value: page.slot.name
    placeholder: I18n.t("answers.slot_new")
    error: item.errors[page.path] || ""
    onEdited: text => page.change("name", text)
  }
  SettingsList {
    Layout.fillWidth: true
    label: I18n.t("contexts.files")
    items: page.slot.files
    placeholder: I18n.t("settings.file_placeholder")
    browse: true
    nameFilters: Eco.textFileFilters()
    onChanged: items => page.change("files", items)
  }
  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("contexts.kinds") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Repeater {
        model: page.config.kinds
        delegate: Chip {
          required property string modelData
          readonly property bool on: page.slot.kinds.includes(modelData)
          text: page.section.kindName(modelData)
          checked: on
          dim: !on
          onClicked: page.change("kinds", on ? page.slot.kinds.filter(kind => kind !== modelData) : page.slot.kinds.concat([modelData]))
        }
      }
    }
  }
  ItemNote { text: I18n.t("answers.slot_note") }
}
