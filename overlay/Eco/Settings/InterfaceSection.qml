pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// InterfaceSection is how eco's windows behave, the [ui] table, one setting a
// row with its value as its control: the interface language a list, and two
// switches, each lit when on. Every change saves at once; Enter or Space on a
// row turns its switch or opens its list. The note says what the row picked does.
SettingsSection {
  id: section
  name: I18n.t("settings.interface")
  note: ({ language: I18n.t("interface.language_tip"), hide_from_share: I18n.t("interface.hide_from_share_tip"),
    tray: I18n.t("interface.tray_tip") })[settings.picked[settings.at.interface] || "language"] || ""
  columns: [{ title: I18n.t("interface.column_setting"), weight: 1 }]
  stacked: [[0]]
  rows: config => ["language", "hide_from_share", "tray"].map(key => ({ id: key, fixed: true, cells: [I18n.t("interface." + key)] }))
  toolsTitle: I18n.t("interface.column_value")
  toolsWidth: 220
  // The row's value: the language's list, or a switch. Bound to the saved
  // config, a save refused leaves it as saved.
  tools: Item {
    id: value
    property var entry: ({})
    readonly property bool language: value.entry.id === "language"
    readonly property bool on: Eco.config !== null && Eco.config.ui[value.entry.id] === true
    function act() {
      if (value.language)
        list.open()
      else
        value.toggle()
    }
    function toggle() {
      const key = value.entry.id
      const on = value.on
      section.settings.saveNow(d => d.ui[key] = !on)
    }
    implicitWidth: section.toolsWidth + 6
    implicitHeight: 28
    Dropdown {
      id: list
      visible: value.language
      anchors.verticalCenter: parent.verticalCenter
      options: ["auto", ...I18n.available.map(pack => pack.code)]
      current: Eco.config !== null ? Eco.config.ui.language : "auto"
      describe: code => section.languageName(code)
      onPicked: code => section.settings.saveNow(d => d.ui.language = code)
    }
    Chip {
      visible: !value.language
      anchors.verticalCenter: parent.verticalCenter
      text: section.settings.onOff(value.on).toUpperCase()
      checked: value.on
      dim: !value.on
      onClicked: value.toggle()
    }
  }

  // A language as its own pack names it; "auto" follows the system.
  function languageName(code) {
    return code === "auto" ? I18n.t("interface.auto") : (I18n.packs[code] ? I18n.packs[code]._name : code)
  }
}
