pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// EchoView is the [audio] table, how your microphone is kept apart from the
// others: each switch beside what it does, and why echo cancellation cannot
// run here when it cannot.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property string unavailable: Eco.echoCancelError ? I18n.t("error." + Eco.echoCancelError) : ""

  title: I18n.t("audio.echo").toUpperCase()
  draft: ItemDraft { id: item; paths: ["audio"] }
  facts: ["drop_echoes", "echo_cancel"].map(key => [I18n.t("audio." + key),
    settings.onOff(section.echoOn(config, key)) + (key === "echo_cancel" && unavailable ? "  ·  " + unavailable : "")])

  Repeater {
    model: ["drop_echoes", "echo_cancel"]
    RowLayout {
      id: option
      required property string modelData
      readonly property bool on: page.section.echoOn(page.config, modelData)
      Layout.fillWidth: true
      spacing: 10
      Chip {
        text: I18n.t("audio." + option.modelData)
        checked: option.on
        dim: !checked
        // The value as clicked, so the change made again sets the same.
        onClicked: {
          const set = { [option.modelData]: !option.on }
          item.update(d => d.audio = Object.assign({ echo_cancel: false, drop_echoes: true }, d.audio, set))
        }
      }
      Label {
        Layout.fillWidth: true
        text: option.modelData === "echo_cancel" && page.unavailable ? page.unavailable : I18n.t("audio." + option.modelData + "_tip")
        color: Theme.dim
        font.pixelSize: 10
        wrapMode: Text.Wrap
      }
    }
  }
}
