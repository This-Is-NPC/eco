pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ModelView is one registered model. Read, what it is — its type, what it is
// the default for, its provider and the provider's id for it, how much a chat
// model reasons, what a minute of audio costs a transcription model — and what
// else uses it, each use a link to where it is set, and the last TEST of it,
// pressed from the bar. Edited: its type while
// nothing uses it, its name, its reasoning, its provider, picked from the
// Providers tab, the id, its price per minute and, for chat, the extra fields
// sent with each request. Renaming it renames it wherever it is used. A new
// model starts from the provider of the last one of its type.
ItemPage {
  id: page

  required property Item settings
  required property SettingsSection section

  readonly property bool fresh: key === "+"
  // Its place in the config being edited: past the end for a new one.
  readonly property int at: item.base === null ? -1 : fresh ? item.base.models.length : item.base.models.findIndex(model => model.name === key)
  readonly property var entry: (item.active ? item.draft.models[at] : config ? config.models.find(model => model.name === key) : null)
    || ({ name: "", type: "chat", provider: "", model: "" })
  readonly property bool chat: entry.type === "chat"
  readonly property var links: config ? section.links(config, entry.name) : []
  // Anything answers with it, a default too: its type stays.
  readonly property bool used: config !== null && Draft.users(config, entry.name).length > 0
  // The prefix of its fields' paths in the draft's errors.
  readonly property string path: "models." + at + "."
  // The name what uses it knows it by in the draft (see Draft.rename): its
  // saved one until another name is its own.
  property string following: ""
  // The extra fields and the price as typed, so what is not saved yet stays.
  property string extraText: ""
  property string priceText: ""

  // The provider-list target of a model type.
  function target(type) { return type === "chat" ? "llm" : "stt" }
  function change(key, value) {
    const at = page.at
    item.update(d => {
      if (value === null)
        delete d.models[at][key]
      else
        d.models[at][key] = value
    })
  }
  // Renames it to `text` in the draft; the change holds where and from what.
  function rename(text) {
    const at = page.at
    const from = following
    let to = from
    item.update(d => to = Draft.rename(d, at, text, from))
    following = to
  }
  // How much it reasons, as read: "" leaves it to the provider, or to a
  // `reasoning` in the extra fields.
  function reasoningName(reasoning) {
    return ({ "": I18n.t(entry.extra && entry.extra.reasoning ? "settings.reasoning_extra" : "settings.reasoning_provider"),
      off: I18n.t("settings.reasoning_off"), low: I18n.t("settings.reasoning_low"), medium: I18n.t("settings.reasoning_medium"),
      high: I18n.t("settings.reasoning_high") })[reasoning]
  }
  // The fields typed show the draft again.
  function reload() {
    extraText = JSON.stringify(entry.extra || {})
    priceText = entry.price_per_minute === undefined ? "" : String(entry.price_per_minute)
    item.type(path + "extra", "")
  }

  kind: I18n.t("settings.item_model")
  title: entry.name
  placeholder: I18n.t("settings.model_new")
  nextKey: entry.name.trim()
  draft: ItemDraft {
    id: item
    paths: page.at >= 0 ? ["models." + page.at] : []
    // Begun or discarded, not rebased: what is typed shows the draft, and what
    // uses it knows it by its saved name.
    onBegan: {
      page.following = page.fresh ? "" : page.key
      page.reload()
    }
  }
  Component.onCompleted: if (fresh) item.begin(d => d.models.push({ name: "", type: "chat", provider: Draft.provider(d, "chat"), model: "" }))
  // Its provider's model list, listed for another model, is asked for again.
  onEditingChanged: if (editing) Eco.clearModels(target(entry.type))

  facts: [
    [I18n.t("settings.model_type"), section.typeName(entry.type)],
    [I18n.t("settings.fact_default"), config ? section.defaultsText(config, entry.name) : ""],
    [I18n.t("models.provider"), entry.provider],
    [I18n.t("settings.column_model_id"), entry.model]
  ].concat(chat ? [[I18n.t("settings.reasoning"), reasoningName(entry.reasoning || "")]].concat(entry.extra ? [[I18n.t("config.extra"), JSON.stringify(entry.extra)]] : [])
    : [[I18n.t("settings.fact_price"), entry.price_per_minute === undefined ? I18n.t("settings.price_unset") : String(entry.price_per_minute)]])
    .concat(section.testFacts(entry.name))
  extra: [section.testCommand(entry.name)]
  related: {
    const groups = []
    links.forEach(link => {
      let group = groups.find(group => group.title === link.group)
      if (!group)
        groups.push(group = { title: link.group, items: [] })
      group.items.push(link)
    })
    if (entry.provider)
      groups.push({ title: settings.sections[settings.at.providers].name, items: [settings.link(settings.at.providers, entry.provider, entry.provider)] })
    return groups
  }

  ColumnLayout {
    visible: !page.used
    spacing: 6
    Caption { text: I18n.t("settings.model_type") }
    Flow {
      spacing: 6
      Repeater {
        model: ["chat", "transcription"]
        delegate: Chip {
          required property string modelData
          text: page.section.typeName(modelData)
          checked: page.entry.type === modelData
          onClicked: if (!checked) {
            const at = page.at
            const type = modelData
            Eco.clearModels(page.target(type))
            item.update(d => Draft.retype(d, at, type))
          }
        }
      }
    }
  }

  Field {
    Layout.fillWidth: true
    label: I18n.t("settings.model_name")
    value: page.entry.name
    placeholder: I18n.t("settings.model_new")
    error: item.errors[page.path + "name"] || ""
    onEdited: text => page.rename(text)
  }

  ColumnLayout {
    visible: page.chat
    spacing: 6
    Caption { text: I18n.t("settings.reasoning") }
    Flow {
      spacing: 6
      Repeater {
        // Sent with every request, but for "".
        model: ["", "off", "low", "medium", "high"]
        delegate: Chip {
          required property string modelData
          text: page.reasoningName(modelData)
          tip: modelData === "off" ? I18n.t("settings.reasoning_off_tip") : ""
          checked: (page.entry.reasoning || "") === modelData
          onClicked: page.change("reasoning", modelData || null)
        }
      }
    }
  }

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("models.provider") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Repeater {
        model: page.config ? Draft.listed(page.config, "providers") : []
        delegate: Chip {
          required property string modelData
          text: modelData
          checked: page.entry.provider === modelData
          onClicked: if (!checked) {
            Eco.clearModels(page.target(page.entry.type))
            page.change("provider", modelData)
          }
        }
      }
    }
    FieldError { Layout.fillWidth: true; text: item.errors[page.path + "provider"] || "" }
    ItemNote { text: I18n.t("models.provider_note") }
  }

  ModelIdFields {
    Layout.fillWidth: true
    target: page.target(page.entry.type)
    error: item.errors[page.path + "model"] || ""
    section: page.entry
    example: Eco.presets.models.find(model => model.type === page.entry.type) || null
    onEdited: (key, value) => page.change(key, value)
  }

  Field {
    Layout.fillWidth: true
    visible: !page.chat
    label: I18n.t("settings.price_per_minute")
    placeholder: I18n.t("settings.price_per_minute_unset")
    value: page.priceText
    input.validator: RegularExpressionValidator { regularExpression: /[0-9]{0,6}([.,][0-9]{0,6})?/ }
    input.inputMethodHints: Qt.ImhFormattedNumbersOnly
    onEdited: text => {
      page.priceText = text
      const price = Number(text.replace(",", "."))
      if (text === "")
        page.change("price_per_minute", null)
      else if (Number.isFinite(price))
        page.change("price_per_minute", price)
    }
  }

  Field {
    Layout.fillWidth: true
    visible: page.chat
    label: I18n.t("config.extra")
    value: page.extraText
    error: item.errors[page.path + "extra"] || ""
    onEdited: text => {
      page.extraText = text
      try {
        const extra = JSON.parse(text)
        if (!extra || typeof extra !== "object" || Array.isArray(extra))
          throw new Error("invalid extra")
        item.type(page.path + "extra", "")
        page.change("extra", extra)
      } catch (error) {
        item.type(page.path + "extra", I18n.t("settings.invalid_extra"))
      }
    }
  }
}
