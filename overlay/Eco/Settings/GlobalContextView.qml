pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// GlobalContextView is the [context] table: the files sent with every answer,
// in every session, a child table whose rows are added, moved and removed,
// each saved at once; beside it the context slots a session turns on, each a
// link. It has no fields to edit.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  kind: I18n.t("settings.answers")
  title: I18n.t("answers.context").toUpperCase()
  details: [ItemNote { text: I18n.t("answers.context_note") }]
  related: [{ title: I18n.t("contexts.slots"),
    items: (config.contexts || []).map(slot => settings.link(settings.at.answers, section.slotId(slot.name), slot.name)) }]
  tables: [section.filesTable(config.context.files, "", addFile)]

  property Component addFile: AddFileScreen { slot: "" }
}
