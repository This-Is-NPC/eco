import QtQuick
import Eco.Core

// ProvidersSection is the services models are served by, one row each: a name,
// a base URL and the secret its requests carry, if any.
// A row opens ProviderView; a provider
// a model uses is not deleted.
SettingsSection {
  id: section

  name: I18n.t("settings.providers")
  note: I18n.t("providers.note")
  columns: [{ title: I18n.t("settings.column_name"), weight: 0.2 }, { title: I18n.t("providers.column_url"), weight: 0.43 },
    { title: I18n.t("providers.column_secret"), weight: 0.17 }, { title: I18n.t("settings.column_used_by"), weight: 0.2 }]
  stacked: [[0, 2], [1], [3]]
  add: I18n.t("providers.add")
  file: "ProviderView.qml"
  rows: config => config.providers.map(provider => {
    const users = Draft.referrers(config, "providers", provider.name)
    return { id: provider.name, removable: users.length === 0,
      keptReason: users.length > 0 ? I18n.t("providers.delete_in_use", { names: users.join(", ") }) : "",
      cells: [provider.name, provider.base_url, provider.secret || I18n.t("providers.no_secret"),
        users.join(", ")] }
  })
  remove: (config, row) => ({
    title: I18n.t("providers.remove_title"),
    question: I18n.t("providers.remove_question", { name: row.id }),
    warning: "",
    change: d => d.providers.splice(d.providers.findIndex(provider => provider.name === row.id), 1)
  })
}
