pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SecretView is one secret: where a key comes from, never the key. Read, its
// source and reference, whether the daemon reads a key from it now, and the
// providers that use it as links. Edited: its name, which the providers follow,
// its source — an environment variable, an omapass account or, on Windows, a
// Credential Manager name — and the reference, typed or picked from the names
// the daemon lists, with how to set a variable so the daemon sees it on this
// system.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  // Its place in the config being edited: past the end for a new one.
  readonly property int at: item.base === null ? -1 : fresh ? item.base.secrets.length : item.base.secrets.findIndex(secret => secret.name === key)
  readonly property var entry: (item.active ? item.draft.secrets[at] : config ? config.secrets.find(secret => secret.name === key) : null)
    || ({ name: "", env: "" })
  readonly property string source: Draft.sourceOf(entry)
  readonly property string reference: section.reference(entry)
  readonly property string path: "secrets." + at + "."
  readonly property var users: config ? Draft.referrers(config, "secrets", entry.name, true) : []
  // The name the providers know it by in the draft (see Draft.renameItem): its
  // saved one until another name is its own.
  property string following: ""
  // Renames it to `text` in the draft; the change holds where and from what.
  function rename(text) {
    const at = page.at
    const from = following
    let to = from
    item.update(d => to = Draft.renameItem(d, "secrets", at, text, from))
    following = to
  }
  // Reads the key from `source`, under `name`, and from nowhere else.
  function choose(source, name) {
    const at = page.at
    item.update(d => {
      const secret = d.secrets[at]
      for (const field of ["env", "omapass", "credential"])
        delete secret[field]
      secret[source] = name
    })
  }
  kind: I18n.t("settings.item_secret")
  title: entry.name
  placeholder: I18n.t("secrets.new")
  nextKey: entry.name.trim()
  draft: ItemDraft {
    id: item
    paths: page.at >= 0 ? ["secrets." + page.at] : []
    // Begun or discarded, not rebased: the providers know it by its saved name.
    onBegan: page.following = page.fresh ? "" : page.key
  }
  Component.onCompleted: if (fresh) item.begin(d => d.secrets.push({ name: "", env: "" }))
  onEditingChanged: if (editing) Eco.requestKeys()

  facts: [
    [I18n.t("secrets.column_source"), section.sourceName(source)],
    [I18n.t("secrets.column_reference"), reference],
    [I18n.t("secrets.column_state"), section.stateText(entry.name)]
  ]
  related: [{ title: settings.sections[settings.at.providers].name,
    items: users.map(name => settings.link(settings.at.providers, name, name)) }]

  Field {
    Layout.fillWidth: true
    label: I18n.t("settings.model_name")
    value: page.entry.name
    placeholder: I18n.t("secrets.new")
    error: item.errors[page.path + "name"] || ""
    onEdited: text => page.rename(text)
  }

  SecretFields {
    Layout.fillWidth: true
    secret: page.entry
    error: item.errors[page.path + "source"] || ""
    onChosen: (source, name) => page.choose(source, name)
  }
}
