pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TranscriptionView is the [stt] table: the model that transcribes sessions
// whose kind picks none, a link to it, and the spoken languages offered, a
// child table whose rows are added, moved, removed and made the one a session
// starts in, each saved at once. Edited, only the model.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property var stt: config.stt

  title: section.name.toUpperCase()
  draft: ItemDraft { id: item; paths: ["stt"] }
  facts: [
    [I18n.t("settings.default_pick"), stt.model],
    [I18n.t("config.current_language"), Eco.languageName(stt.language)]
  ]
  details: [ItemNote { text: I18n.t("settings.default_transcription_tip") }]
  related: [{ title: settings.sections[settings.at.models].name,
    items: stt.model ? [settings.link(settings.at.models, stt.model, stt.model)] : [] }]

  tables: [{
    name: "languages",
    title: I18n.t("config.languages"),
    // A language reads as its code and its name.
    columns: [{ title: I18n.t("settings.column_language"), weight: 0.7 }, { title: I18n.t("settings.language_default"), weight: 0.3 }],
    stacked: [[0, 1]],
    tone: (entry, column) => column === 1 ? Theme.primary : "",
    rows: stt.languages.map(code => ({ id: code, cells: [Eco.languageName(code), code === stt.language ? I18n.t("settings.language_default") : ""],
      removable: stt.languages.length > 1, keptReason: stt.languages.length > 1 ? "" : I18n.t("settings.language_last") })),
    add: { text: I18n.t("settings.add_language"), tip: I18n.t("settings.add_language_tip"), screen: addLanguage },
    remove: row => {
      const heir = Draft.languageHeir(Eco.config, row.id)
      return {
        title: I18n.t("settings.remove_language_title"),
        question: heir ? I18n.t("settings.remove_language_question", { name: Eco.languageName(row.id) }) : "",
        warning: heir ? I18n.t("settings.remove_language_default", { name: Eco.languageName(heir) }) : "",
        change: d => Draft.removeLanguage(d, row.id)
      }
    },
    move: { up: I18n.t("settings.language_move_up_tip"), down: I18n.t("settings.language_move_down_tip"),
      change: (d, row, step) => Draft.moveLanguage(d, row.id, step) },
    makeDefault: { is: row => row.id === stt.language, tip: I18n.t("settings.language_default_tip"),
      isTip: I18n.t("settings.language_is_default"), change: (d, row) => d.stt.language = row.id }
  }]

  // The languages eco knows that are not offered yet, to add one.
  property Component addLanguage: ChildScreen {
    title: I18n.t("settings.add_language")
    note: I18n.t("settings.add_language_note")
    columns: [{ title: I18n.t("settings.column_language"), weight: 1 }]
    rows: ["auto", ...Eco.languageCodes].filter(code => !Eco.config.stt.languages.includes(code))
      .map(code => ({ id: code, cells: [Eco.languageName(code)] }))
    empty: I18n.t("settings.add_language_empty")
    chooseText: I18n.t("settings.add_language")
    chooseTip: I18n.t("settings.add_language_choose_tip")
    choose: row => ({ change: d => Draft.addLanguage(d, row.id), pick: row.id })
  }

  PickRow {
    Layout.fillWidth: true
    text: I18n.t("settings.default_pick")
    options: Draft.named(page.config, "transcription")
    current: page.stt.model
    label: current ? "" : I18n.t("settings.pick_model")
    invalid: item.errors["stt.model"] !== undefined
    onPicked: name => item.update(d => d.stt.model = name)
  }
  FieldError { Layout.fillWidth: true; text: item.errors["stt.model"] || "" }
  FieldError { Layout.fillWidth: true; text: item.errors["stt.languages"] || "" }
  ItemNote { text: I18n.t("settings.default_transcription_tip") }
}
