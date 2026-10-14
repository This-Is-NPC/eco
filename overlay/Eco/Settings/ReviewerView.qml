pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ReviewerView is the reviewer: whether a second model rewrites every answer
// before it shows, with which model and what it is asked, and whether the
// draft it rewrote is kept to inspect. Turned on with no model, it takes the
// assistant's.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property var reviewer: config.reviewer || ({ enabled: false, model: "" })
  readonly property bool on: reviewer.enabled === true

  kind: I18n.t("settings.answers")
  title: I18n.t("answers.reviewer")
  draft: ItemDraft { id: item; paths: ["reviewer"] }
  facts: [[I18n.t("settings.reviewer_on"), settings.onOff(on)]]
    .concat(on ? [[I18n.t("settings.reviewer_model"), reviewer.model], [I18n.t("settings.reviewer_verbose"), settings.onOff(reviewer.verbose === true)]] : [])
  details: [
    ItemNote { text: I18n.t("settings.reviewer_note") },
    Caption { visible: !!page.reviewer.prompt; text: I18n.t("settings.reviewer_prompt").toUpperCase() },
    Label { Layout.fillWidth: true; visible: !!page.reviewer.prompt; text: page.reviewer.prompt || ""; font.pixelSize: 12; wrapMode: Text.Wrap }
  ]
  related: [{ title: settings.sections[settings.at.models].name,
    items: reviewer.model ? [settings.link(settings.at.models, reviewer.model, reviewer.model)] : [] }]

  ItemNote { text: I18n.t("settings.reviewer_note") }
  Flow {
    Layout.fillWidth: true
    spacing: 6
    Chip {
      text: I18n.t("settings.reviewer_on")
      checked: page.on
      // The values as clicked, so the change made again sets the same.
      onClicked: {
        const on = !page.on
        item.update(d => Draft.review(d, on))
      }
    }
    Chip {
      visible: page.on
      text: I18n.t("settings.reviewer_verbose")
      tip: I18n.t("settings.reviewer_verbose_tip")
      checked: page.reviewer.verbose === true
      onClicked: {
        const verbose = page.reviewer.verbose !== true
        item.update(d => d.reviewer.verbose = verbose)
      }
    }
  }
  PickRow {
    Layout.fillWidth: true
    visible: page.on
    text: I18n.t("settings.reviewer_model")
    options: Draft.named(page.config, "chat")
    current: page.reviewer.model
    label: current ? "" : I18n.t("settings.pick_model")
    invalid: item.errors["reviewer.model"] !== undefined
    onPicked: name => item.update(d => d.reviewer.model = name)
  }
  FieldError { Layout.fillWidth: true; text: item.errors["reviewer.model"] || "" }
  SettingsTextArea {
    Layout.fillWidth: true
    visible: page.on
    label: I18n.t("settings.reviewer_prompt")
    value: page.reviewer.prompt || ""
    onEdited: text => item.update(d => d.reviewer.prompt = text)
  }
}
