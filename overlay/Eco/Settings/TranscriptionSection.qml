import QtQuick
import Eco.Core

// TranscriptionSection is the spoken languages offered when a session starts,
// one row each, the default marked. ADD LANGUAGE chooses one more on
// AddLanguageView; the picked one is made the default or deleted from the bar,
// saved at once. The last one stays; deleting the default makes the first one
// left the default. A language has no screen of its own.
SettingsSection {
  name: I18n.t("settings.transcription")
  note: I18n.t("settings.transcription_note")
  columns: [{ title: I18n.t("settings.column_language"), weight: 0.7 }, { title: I18n.t("settings.language_default"), weight: 0.3 }]
  stacked: [[0, 1]]
  add: I18n.t("settings.add_language")
  adds: "AddLanguageView.qml"
  rows: config => config.stt.languages.map(code => ({ id: code,
    cells: [Eco.languageName(code), code === config.stt.language ? I18n.t("settings.language_default") : ""],
    removable: config.stt.languages.length > 1, keptReason: config.stt.languages.length > 1 ? "" : I18n.t("settings.language_last") }))
  remove: (config, row) => {
    const heir = Draft.languageHeir(config, row.id)
    return {
      title: I18n.t("settings.remove_language_title"),
      question: I18n.t("settings.remove_language_question", { name: Eco.languageName(row.id) }),
      warning: heir ? I18n.t("settings.remove_language_default", { name: Eco.languageName(heir) }) : "",
      change: d => Draft.removeLanguage(d, row.id)
    }
  }
  commands: (config, row) => {
    const is = row !== null && row.id === config.stt.language
    return [{ text: I18n.t("settings.default"), icon: "check", tip: I18n.t(is ? "settings.language_is_default" : "settings.language_default_tip"),
      shown: row !== null, enabled: !is && !Eco.configSaving, run: () => settings.saveNow(d => d.stt.language = row.id) }]
  }
}
