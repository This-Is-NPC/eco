pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// PromptView is one prompt of the Prompts table: read, what it is for and its
// text as written; edited, the text.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property string text: section.text(config, key)

  kind: I18n.t("prompts.item")
  title: section.title(key)
  draft: ItemDraft { id: item; paths: [page.section.path(page.key)] }
  details: [
    ItemNote { text: I18n.t(page.key === "rules" ? "prompts.rules_note" : "prompts.reviewer_note") },
    ItemNote { visible: page.text.trim() === ""; text: I18n.t("prompts.empty") },
    ItemText { visible: page.text.trim() !== ""; title: I18n.t("prompts.column_text"); text: page.text }
  ]

  SettingsTextArea {
    Layout.fillWidth: true
    label: page.title
    value: page.text
    onEdited: text => item.update(d => page.section.write(d, page.key, text))
  }
}
