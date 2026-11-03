pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ProviderView is one provider: a service models are served by. Read, its base
// URL, the secret its requests carry and whether that resolves now, and the
// models served by it as links. Edited: its name, which the models follow, its
// base URL — an OpenAI-compatible endpoint, or for transcription a streaming
// one by its wss:// URL — and its secret, picked from the Secrets tab or none.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  // Its place in the config being edited: past the end for a new one.
  readonly property int at: item.base === null ? -1 : fresh ? item.base.providers.length : item.base.providers.findIndex(provider => provider.name === key)
  readonly property var entry: (item.active ? item.draft.providers[at] : config ? config.providers.find(provider => provider.name === key) : null)
    || ({ name: "", base_url: "" })
  readonly property string path: "providers." + at + "."
  readonly property var users: config ? Draft.referrers(config, "providers", entry.name, true) : []
  // The name the models know it by in the draft (see Draft.renameItem): its
  // saved one until another name is its own.
  property string following: ""

  function change(key, value) {
    const at = page.at
    item.update(d => {
      if (value === null)
        delete d.providers[at][key]
      else
        d.providers[at][key] = value
    })
  }
  // Renames it to `text` in the draft; the change holds where and from what.
  function rename(text) {
    const at = page.at
    const from = following
    let to = from
    item.update(d => to = Draft.renameItem(d, "providers", at, text, from))
    following = to
  }

  kind: I18n.t("settings.item_provider")
  title: entry.name
  placeholder: I18n.t("providers.new")
  nextKey: entry.name.trim()
  draft: ItemDraft {
    id: item
    paths: page.at >= 0 ? ["providers." + page.at] : []
    // Begun or discarded, not rebased: the models know it by its saved name.
    onBegan: page.following = page.fresh ? "" : page.key
  }
  Component.onCompleted: if (fresh) item.begin(d => d.providers.push({ name: "", base_url: "" }))

  facts: [
    [I18n.t("provider.base_url"), entry.base_url],
    [I18n.t("providers.column_secret"), entry.secret ? entry.secret + " · " + page.stateOf(entry.secret) : I18n.t("providers.no_secret")]
  ]
  related: [{ title: settings.sections[settings.at.secrets].name,
    items: entry.secret ? [settings.link(settings.at.secrets, entry.secret, entry.secret)] : [] },
    { title: settings.sections[settings.at.models].name,
      items: users.map(name => settings.link(settings.at.models, name, name)) }]

  function stateOf(secret) { return settings.sections[settings.at.secrets].stateText(secret) }

  Field {
    Layout.fillWidth: true
    label: I18n.t("settings.model_name")
    value: page.entry.name
    placeholder: I18n.t("providers.new")
    error: item.errors[page.path + "name"] || ""
    onEdited: text => page.rename(text)
  }

  Field {
    Layout.fillWidth: true
    label: I18n.t("provider.base_url")
    value: page.entry.base_url
    placeholder: I18n.t("providers.url_placeholder")
    error: item.errors[page.path + "base_url"] || ""
    onEdited: text => page.change("base_url", text)
  }

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("providers.column_secret") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Chip {
        text: I18n.t("providers.no_secret")
        checked: !page.entry.secret
        onClicked: page.change("secret", null)
      }
      Repeater {
        model: page.config ? Draft.listed(page.config, "secrets") : []
        delegate: Chip {
          required property string modelData
          text: modelData
          checked: page.entry.secret === modelData
          onClicked: page.change("secret", modelData)
        }
      }
    }
    FieldError { Layout.fillWidth: true; text: item.errors[page.path + "secret"] || "" }
    ItemNote { text: I18n.t("providers.secret_note") }
  }
}
