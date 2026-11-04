import QtQuick
import Eco.Core

// SecretsSection is where each key comes from, one row each: a name, the kind
// of source — an environment variable, an omapass account or, on Windows, a
// Credential Manager name — the reference, and whether the daemon reads a key
// from it now. A reference only: eco never creates or stores a key. The
// secrets eco ships follow the user's, as presets. A row opens SecretView; a
// secret a provider uses is not deleted.
SettingsSection {
  id: section

  name: I18n.t("settings.secrets")
  note: I18n.t("secrets.note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.2 }, { title: I18n.t("secrets.column_source"), weight: 0.17 },
    { title: I18n.t("secrets.column_reference"), weight: 0.27 }, { title: I18n.t("secrets.column_state"), weight: 0.36 }]
  stacked: [[0, 1], [2], [3]]
  add: I18n.t("secrets.add")
  file: "SecretView.qml"
  rows: config => config.secrets.map(secret => {
    const users = Draft.referrers(config, "secrets", secret.name)
    return { id: secret.name, removable: users.length === 0,
      keptReason: users.length > 0 ? I18n.t("secrets.delete_in_use", { names: users.join(", ") }) : "",
      cells: [secret.name, sourceName(Draft.sourceOf(secret)), reference(secret), stateText(secret.name)] }
  }).concat(Draft.shipped(config, "secrets").map(preset => ({
    id: preset.name, file: "PresetView.qml", fixed: true, edits: false, noEdit: I18n.t("presets.no_edit"), dim: true,
    cells: [preset.name, sourceName("env"), preset.env, stateText(preset.name)] })))
  remove: (config, row) => ({
    title: I18n.t("secrets.remove_title"),
    question: I18n.t("secrets.remove_question", { name: row.id }),
    warning: I18n.t("secrets.remove_note"),
    change: d => d.secrets.splice(d.secrets.findIndex(secret => secret.name === row.id), 1)
  })
  // CHECK reads every secret again and tells what it found for the picked one:
  // omapass and the Credential Manager change while eco runs; a variable does
  // not, for the daemon reads its environment once. Busy until the reply.
  commands: (config, row) => [{ text: I18n.t("secrets.check"), icon: "check", tip: I18n.t("secrets.check_tip"),
    busy: Eco.secretChecking !== "" && Eco.secretChecking === row.id,
    enabled: Eco.connected, run: () => Eco.checkSecret(row.id) }]

  function sourceName(source) { return I18n.t(Draft.sourceKey(source)) }
  function reference(secret) { return secret[Draft.sourceOf(secret)] || "" }
  // Whether the secret `name` resolves now, and if not, why, as the daemon told it.
  function stateText(name) {
    const state = Eco.secretStatus[name]
    if (state === undefined)
      return I18n.t("secrets.unknown")
    if (state.resolved)
      return I18n.t("secrets.resolved")
    return I18n.t("error." + state.code, state.params)
  }
  // The screen of the preset secret `name`.
  function presetFacts(name) {
    const preset = Eco.presets.secrets.find(entry => entry.name === name)
    return { facts: [[I18n.t("secrets.column_source"), sourceName("env")], [I18n.t("secrets.column_reference"), preset.env],
      [I18n.t("secrets.column_state"), stateText(name)]], disabled: "" }
  }
}
