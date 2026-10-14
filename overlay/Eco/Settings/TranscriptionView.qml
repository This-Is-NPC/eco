pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TranscriptionView is the [stt] table: the model that transcribes sessions
// whose kind picks none, a link to it, the spoken languages offered and the
// one a session starts in.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property var stt: config.stt

  kind: I18n.t("section.config")
  title: section.name.toUpperCase()
  draft: ItemDraft { id: item; paths: ["stt"] }
  facts: [
    [I18n.t("settings.default_pick"), stt.model],
    [I18n.t("config.languages"), stt.languages.map(code => Eco.languageName(code)).join("\n")],
    [I18n.t("config.current_language"), Eco.languageName(stt.language)]
  ]
  details: [ItemNote { text: I18n.t("settings.default_transcription_tip") }]
  related: [{ title: settings.sections[settings.at.models].name,
    items: stt.model ? [settings.link(settings.at.models, stt.model, stt.model)] : [] }]

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
  ItemNote { text: I18n.t("settings.default_transcription_tip") }

  SettingsList {
    Layout.fillWidth: true
    label: I18n.t("config.languages")
    items: page.stt.languages
    error: item.errors["stt.languages"] || ""
    choices: ["auto", ...Eco.languageCodes]
    describe: code => Eco.languageName(code)
    onChanged: items => item.update(d => {
      d.stt.languages = items.map(code => code.toLowerCase())
      if (!d.stt.languages.includes(d.stt.language) && d.stt.languages.length)
        d.stt.language = d.stt.languages[0]
    })
  }
  PickRow {
    text: I18n.t("config.current_language")
    options: page.stt.languages
    current: page.stt.language
    describe: code => Eco.languageName(code)
    onPicked: code => item.update(d => d.stt.language = code)
  }
}
