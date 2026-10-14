import QtQuick
import Eco.Core

// AddLanguageView chooses a spoken language to offer from those eco knows that
// are not offered yet; once saved it is picked on the Transcription table.
ChooseView {
  title: I18n.t("settings.add_language")
  note: I18n.t("settings.add_language_note")
  columns: [{ title: I18n.t("settings.column_language"), weight: 1 }]
  rows: ["auto", ...Eco.languageCodes].filter(code => !Eco.config.stt.languages.includes(code))
    .map(code => ({ id: code, cells: [Eco.languageName(code)] }))
  empty: I18n.t("settings.add_language_empty")
  chooseText: I18n.t("settings.add_language")
  chooseIcon: "plus"
  chooseTip: I18n.t("settings.add_language_choose_tip")
  choose: row => ({ change: d => Draft.addLanguage(d, row.id), pick: row.id })
}
