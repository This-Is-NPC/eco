pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// GeneralSection is how eco behaves, one setting a row under the title of
// what it is about, its value as its control: the interface language a list,
// numbers a field, the rest switches, each lit when on. Every change saves at
// once; Enter or Space on a row turns its switch, opens its list or starts
// writing its number. The note says what the row picked does.
SettingsSection {
  id: section

  // The rows, by group: [group key, [setting ids]].
  readonly property var groups: [
    ["interface", ["language", "hide_from_share", "tray"]],
    ["reviewer", ["review", "show_draft"]],
    ["audio", ["drop_echoes", "echo_cancel"]],
    ["answers", ["max_context_chars", "concurrency"]]
  ]
  // Why echo cancellation cannot run here, or "".
  readonly property string echoUnavailable: Eco.echoCancelError ? I18n.t("error." + Eco.echoCancelError) : ""
  readonly property var tips: ({
    language: I18n.t("interface.language_tip"),
    hide_from_share: I18n.t("interface.hide_from_share_tip"),
    tray: I18n.t("interface.tray_tip"),
    review: I18n.t("general.review_tip"),
    show_draft: I18n.t("general.show_draft_tip"),
    drop_echoes: I18n.t("audio.drop_echoes_tip"),
    echo_cancel: echoUnavailable || I18n.t("audio.echo_cancel_tip"),
    max_context_chars: I18n.t("general.context_tip"),
    concurrency: I18n.t("general.concurrency_tip")
  })
  readonly property var labels: ({
    language: I18n.t("interface.language"),
    hide_from_share: I18n.t("interface.hide_from_share"),
    tray: I18n.t("interface.tray"),
    review: I18n.t("general.review"),
    show_draft: I18n.t("general.show_draft"),
    drop_echoes: I18n.t("audio.drop_echoes"),
    echo_cancel: I18n.t("audio.echo_cancel"),
    max_context_chars: I18n.t("general.context"),
    concurrency: I18n.t("general.concurrency")
  })
  readonly property var titles: ({
    interface: I18n.t("general.group_interface"),
    reviewer: I18n.t("general.group_reviewer"),
    audio: I18n.t("general.group_audio"),
    answers: I18n.t("general.group_answers")
  })

  name: I18n.t("settings.general")
  note: tips[settings.picked[settings.at.general] || "language"] || ""
  columns: [{ title: I18n.t("interface.column_setting"), weight: 1 }]
  stacked: [[0]]
  group: "group"
  rows: config => groups.reduce((all, entry) => all.concat(entry[1].map(key => ({ id: key, fixed: true, group: titles[entry[0]],
    cells: [labels[key]], dim: key === "show_draft" && !isOn(config, "review") }))), [])
  toolsTitle: I18n.t("interface.column_value")
  toolsWidth: 220
  // The row's value: the language's list, a number's field, or a switch.
  // Bound to the saved config, a save refused leaves it as saved.
  tools: Item {
    id: value
    property var entry: ({})
    readonly property string key: value.entry.id || ""
    readonly property bool language: key === "language"
    readonly property bool counts: key === "max_context_chars" || key === "concurrency"
    readonly property bool on: Eco.config !== null && section.isOn(Eco.config, key)
    function act() {
      if (value.language)
        list.open()
      else if (value.counts)
        field.act()
      else
        toggle.press()
    }
    implicitWidth: section.toolsWidth + 6
    implicitHeight: 30
    Dropdown {
      id: list
      visible: value.language
      anchors.verticalCenter: parent.verticalCenter
      options: ["auto", ...I18n.available.map(pack => pack.code)]
      current: Eco.config !== null ? Eco.config.ui.language : "auto"
      describe: code => section.languageName(code)
      onPicked: code => section.settings.saveNow(d => d.ui.language = code)
    }
    NumberField {
      id: field
      visible: value.counts
      anchors.verticalCenter: parent.verticalCenter
      width: 120
      inline: true
      number: Eco.config !== null && value.counts ? Eco.config.llm[value.key] : 0
      onNumberEdited: count => {
        const key = value.key
        section.settings.saveNow(d => d.llm[key] = count)
      }
    }
    Row {
      visible: !value.language && !value.counts
      anchors.verticalCenter: parent.verticalCenter
      spacing: 10
      Chip {
        id: toggle
        text: section.settings.onOff(value.on).toUpperCase()
        checked: value.on
        dim: !value.on
        // Off while it cannot run here, unless it is on: it can always be turned off.
        reason: value.key === "echo_cancel" && !value.on ? section.echoUnavailable : ""
        onClicked: section.toggle(value.key, value.on)
      }
      // The reviewer's model, the one it reviews with when on.
      Label {
        anchors.verticalCenter: parent.verticalCenter
        visible: text !== ""
        width: section.toolsWidth - toggle.width - 10
        text: value.key === "review" && Eco.config !== null ? (Eco.config.reviewer || {}).model || "" : ""
        color: Theme.dim
        font.pixelSize: 10
        elide: Text.ElideRight
      }
    }
  }

  // Whether the switch `key` is on in `config`, with the daemon's defaults.
  function isOn(config, key) {
    const reviewer = config.reviewer || {}
    const audio = Object.assign({ echo_cancel: false, drop_echoes: true }, config.audio)
    return ({
      hide_from_share: config.ui.hide_from_share === true,
      tray: config.ui.tray === true,
      review: reviewer.enabled === true,
      show_draft: reviewer.verbose === true,
      drop_echoes: audio.drop_echoes === true,
      echo_cancel: audio.echo_cancel === true
    })[key] === true
  }
  // Turns the switch `key`, `on` as read, the other way, saved at once;
  // reviewing with no model of its own takes the chat default.
  function toggle(key, on) {
    settings.saveNow(d => {
      if (key === "review")
        Draft.review(d, !on)
      else if (key === "show_draft")
        d.reviewer = Object.assign({ enabled: false, model: "" }, d.reviewer, { verbose: !on })
      else if (key === "drop_echoes" || key === "echo_cancel")
        d.audio = Object.assign({ echo_cancel: false, drop_echoes: true }, d.audio, { [key]: !on })
      else
        d.ui[key] = !on
    })
  }
  // A language as its own pack names it; "auto" follows the system.
  function languageName(code) {
    return code === "auto" ? I18n.t("interface.auto") : (I18n.packs[code] ? I18n.packs[code]._name : code)
  }
}
