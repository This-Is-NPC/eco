import QtQuick
import Eco.Core

// InterfaceSection is how eco's windows behave, the [ui] table: the interface
// language, which CHANGE chooses on InterfaceLanguageView, and two switches,
// each turned on or off from the bar. Every change saves at once. The note
// says what the row picked does.
SettingsSection {
  id: section
  name: I18n.t("settings.interface")
  note: ({ language: I18n.t("interface.language_tip"), hide_from_share: I18n.t("interface.hide_from_share_tip"),
    tray: I18n.t("interface.tray_tip") })[settings.picked[settings.at.interface] || "language"] || ""
  columns: [
    { title: I18n.t("interface.column_setting"), weight: 0.6 },
    { title: I18n.t("interface.column_value"), weight: 0.4 }
  ]
  stacked: [[0, 1]]
  rows: config => [
    { id: "language", fixed: true, file: "InterfaceLanguageView.qml",
      cells: [I18n.t("interface.language"), section.languageName(config.ui.language)],
      open: { text: I18n.t("interface.change"), icon: "edit", role: "primary", tip: I18n.t("interface.change_tip") } },
    section.toggle(config, "hide_from_share"),
    section.toggle(config, "tray")
  ]

  // A language as its own pack names it; "auto" follows the system.
  function languageName(code) {
    return code === "auto" ? I18n.t("interface.auto") : (I18n.packs[code] ? I18n.packs[code]._name : code)
  }
  // The row of the switch `key` of [ui]: its command turns it the other way.
  function toggle(config, key) {
    const on = config.ui[key] === true
    return { id: key, fixed: true,
      cells: [I18n.t("interface." + key), settings.onOff(on)],
      open: { text: I18n.t(on ? "interface.turn_off" : "interface.turn_on"), icon: on ? "close" : "check", role: "primary",
        tip: I18n.t(on ? "interface.turn_off_tip" : "interface.turn_on_tip"), run: () => settings.saveNow(d => d.ui[key] = !on, () => Qt.callLater(settings.takeFocus)) } }
  }
}
