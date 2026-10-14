pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// GlobalContextView is the [context] table: the files sent with every answer,
// in every session, beside the context slots a session turns on, each a link.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section


  kind: I18n.t("settings.answers")
  title: I18n.t("answers.context")
  draft: ItemDraft { id: item; paths: ["context"] }
  facts: [[I18n.t("contexts.files"), config.context.files.join("\n")]]
  details: [ItemNote { text: I18n.t("answers.context_note") }]
  related: [{ title: I18n.t("contexts.slots"),
    items: (config.contexts || []).map(slot => settings.link(settings.at.answers, section.slotId(slot.name), slot.name)) }]

  ItemNote { text: I18n.t("answers.context_note") }
  SettingsList {
    Layout.fillWidth: true
    label: I18n.t("contexts.files")
    items: page.config.context.files
    placeholder: I18n.t("settings.file_placeholder")
    browse: true
    nameFilters: Eco.textFileFilters()
    onChanged: items => item.update(d => d.context.files = items)
  }
}
