import QtQuick
import Eco.Core

// InterfaceLanguageView chooses the interface language: automatic, then each
// language pack by its own name, the one in use marked.
ChooseView {
  id: view

  readonly property string current: Eco.config.ui.language

  title: I18n.t("interface.language")
  note: I18n.t("interface.language_tip")
  columns: [{ title: I18n.t("settings.column_language"), weight: 0.7 }, { title: "", weight: 0.3 }]
  stacked: [[0, 1]]
  rows: ["auto", ...I18n.available.map(pack => pack.code)].map(code => ({ id: code,
    cells: [view.section.languageName(code), code === view.current ? I18n.t("interface.in_use") : ""] }))
  chooseText: I18n.t("interface.choose")
  chooseIcon: "check"
  chooseTip: I18n.t("interface.choose_tip")
  picked: current
  choose: row => ({ change: row.id === view.current ? null : d => d.ui.language = row.id })
}
