import QtQuick
import Eco.Core

// AudioSection says who is heard through which device: AudioBody lists every
// device with the audio source it is heard as and its colour, then the audio
// sources, named and marked as you in their rows. Every change saves at once.
// The bar refreshes the devices and deletes the source picked, saying first
// what stops being heard.
SettingsSection {
  id: section

  name: I18n.t("settings.audio")
  note: I18n.t("audio.note")
  body: AudioBody {}
  // The audio sources, which DELETE removes; a device's row is AudioBody's own.
  rows: config => config.participants.map(person => ({ id: sourceId(person.name), name: person.name, user: !!person.user, cells: ["", ""] }))
  remove: (config, row) => {
    const index = config.participants.findIndex(person => sourceId(person.name) === row.id)
    const lost = Draft.sourceRemoval(config, index, Eco.devices)
    return {
      title: I18n.t("audio.remove_title"),
      question: I18n.t("audio.remove_question", { name: config.participants[index].name }),
      warning: (lost.unheard.length ? [I18n.t("audio.remove_unheard", { devices: lost.unheard.map(id => label(id)).join(", ") })] : [])
        .concat(lost.noOutput ? [I18n.t("audio.remove_no_output")] : []).concat(lost.noUser ? [I18n.t("audio.remove_no_user")] : []).join("\n"),
      change: d => d.participants.splice(d.participants.findIndex(person => sourceId(person.name) === row.id), 1)
    }
  }
  commands: (config, row) => [{ text: I18n.t("audio.refresh_devices"), tip: I18n.t("audio.refresh_tip"), run: () => Eco.requestDevices() }]

  // A source's row id, and a device's: the two tables never share one.
  function sourceId(name) { return "source:" + name }
  function deviceId(id) { return "device:" + id }
  function device(id) { return Eco.devices.find(device => device.id === id) || null }
  // What a device is called; its id when it is not there.
  function label(id) {
    const found = device(id)
    return found ? Eco.deviceLabel(id, found.label) : id
  }
  // Every device there is, then those a source names that are not there.
  function deviceRows(config) {
    const named = config.participants.reduce((all, person) => all.concat(person.devices), [])
    const absent = named.filter((id, i) => named.indexOf(id) === i && device(id) === null)
    return Eco.devices.map(found => ({ id: deviceId(found.id), device: found.id, missing: false,
        cells: [label(found.id), I18n.t(found.kind === "input" ? "audio.kind_input" : "audio.kind_output"), "", "", ""] }))
      .concat(absent.map(id => ({ id: deviceId(id), device: id, missing: true, cells: [id, I18n.t("audio.unavailable"), "", "", ""] })))
  }
}
