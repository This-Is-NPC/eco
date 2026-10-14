pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// AssistantView is the assistant: the chat model that answers questions and
// whatever picks no model of its own, the rules every answer follows, how much
// context it takes and how many answers run at once. Related: its model, and
// the skills, session kinds and translation that answer with it.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property var inheriting: Draft.inheriting(config)
  readonly property var at: settings.at

  kind: I18n.t("settings.answers")
  title: I18n.t("answers.assistant").toUpperCase()
  draft: ItemDraft { id: item; paths: ["llm", "rules"] }
  facts: [
    [I18n.t("settings.default_pick"), config.llm.model],
    [I18n.t("answers.fact_context"), I18n.t("answers.chars", { n: config.llm.max_context_chars })],
    [I18n.t("answers.fact_concurrency"), I18n.t("answers.running", { n: config.llm.concurrency })]
  ]
  details: [
    ItemNote { text: I18n.t("settings.default_assistant_tip") },
    ItemText { visible: page.config.rules.trim() !== ""; title: I18n.t("settings.rules"); text: page.config.rules }
  ]
  related: [
    { title: settings.sections[at.models].name, items: config.llm.model ? [settings.link(at.models, config.llm.model, config.llm.model)] : [] },
    { title: settings.sections[at.skills].name, items: inheriting.skills.map(name => settings.link(at.skills, name, name)) },
    { title: settings.sections[at.sessions].name, items: inheriting.kinds.map(kind => settings.link(at.sessions, kind, section.kindName(kind))) },
    { title: settings.sections[at.translation].name, items: inheriting.translation ? [settings.link(at.translation, "", I18n.t("settings.translation"))] : [] }
  ]

  PickRow {
    Layout.fillWidth: true
    text: I18n.t("settings.default_pick")
    options: Draft.named(page.config, "chat")
    current: page.config.llm.model
    label: current ? "" : I18n.t("settings.pick_model")
    invalid: item.errors["llm.model"] !== undefined
    onPicked: name => item.update(d => d.llm.model = name)
  }
  FieldError { Layout.fillWidth: true; text: item.errors["llm.model"] || "" }
  ItemNote { text: I18n.t("settings.default_assistant_tip") }
  SettingsTextArea {
    Layout.fillWidth: true
    label: I18n.t("settings.rules")
    value: page.config.rules
    onEdited: text => item.update(d => d.rules = text)
  }
  NumberField {
    Layout.fillWidth: true
    label: I18n.t("config.context")
    number: page.config.llm.max_context_chars
    onNumberEdited: number => item.update(d => d.llm.max_context_chars = number)
  }
  NumberField {
    Layout.fillWidth: true
    label: I18n.t("config.concurrency")
    number: page.config.llm.concurrency
    onNumberEdited: number => item.update(d => d.llm.concurrency = number)
  }
}
