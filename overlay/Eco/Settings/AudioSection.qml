import QtQuick
import Eco.Core

// AudioSection says who is heard through which device: each row an audio
// source (a `participants` entry) with its devices and whether it is you,
// opening SourceView, and last how your microphone is kept apart, EchoView,
// which is never deleted. Deleting a source first says what stops being heard.
SettingsSection {
  id: section

  name: I18n.t("settings.audio")
  note: I18n.t("audio.note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.28 }, { title: I18n.t("audio.column_devices"), weight: 0.6 },
    { title: I18n.t("audio.column_you"), weight: 0.12 }]
  stacked: [[0, 2], [1]]
  add: I18n.t("audio.add_source")
  file: "SourceView.qml"
  rows: config => config.participants.map(person => ({ id: sourceId(person.name),
      cells: [person.name, person.devices.map(id => deviceLabel(id)).join(", ") || I18n.t("settings.no_device"), person.user ? I18n.t("audio.you") : ""] }))
    .concat([{ id: "echo", file: "EchoView.qml", fixed: true,
      cells: [I18n.t("audio.echo"), [["drop_echoes", echoOn(config, "drop_echoes")], ["echo_cancel", echoOn(config, "echo_cancel")]]
        .map(([key, on]) => I18n.t("audio." + key) + " " + settings.onOff(on)).join("  ·  "), ""] }])
  remove: (config, row) => {
    const index = config.participants.findIndex(person => sourceId(person.name) === row.id)
    const lost = Draft.sourceRemoval(config, index, Eco.devices)
    return {
      title: I18n.t("audio.remove_title"),
      question: I18n.t("audio.remove_question", { name: config.participants[index].name }),
      warning: (lost.unheard.length ? [I18n.t("audio.remove_unheard", { devices: lost.unheard.map(id => device(id) ? deviceLabel(id) : id).join(", ") })] : [])
        .concat(lost.noOutput ? [I18n.t("audio.remove_no_output")] : []).concat(lost.noUser ? [I18n.t("audio.remove_no_user")] : []).join("\n"),
      change: d => d.participants.splice(d.participants.findIndex(person => sourceId(person.name) === row.id), 1)
    }
  }
  commands: (config, row) => [{ text: I18n.t("audio.refresh_devices"), tip: I18n.t("audio.refresh_tip"), run: () => Eco.requestDevices() }]

  // A source's row id: apart from the echo row, whatever it is named.
  function sourceId(name) { return "source:" + name }
  // A switch of the [audio] table, as the daemon reads it when it is not set.
  function echoOn(config, key) {
    const audio = config.audio || {}
    return key === "drop_echoes" ? audio.drop_echoes !== false : audio.echo_cancel === true
  }
  function device(id) { return Eco.devices.find(device => device.id === id) || null }
  // What a device is called, or that it is not there.
  function deviceLabel(id) {
    const found = device(id)
    return found ? Eco.deviceLabel(id, found.label) : I18n.t("settings.device_missing", { id: id })
  }
  // A device with its kind, and who has it when that is not the source at `except` of `participants`.
  function deviceName(participants, id, except) {
    const found = device(id)
    const kind = found ? I18n.t(found.kind === "input" ? "audio.kind_input" : "audio.kind_output") + "  " : ""
    const owner = participants.find((person, i) => i !== except && person.devices.includes(id))
    return kind + deviceLabel(id) + (owner ? "  ·  " + owner.name : "")
  }
}
