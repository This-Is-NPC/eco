pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ModelSettings registers the models, chat and transcription, as cards that say
// what each model is: closed, its type, the provider's id and what uses it; open,
// what uses it in full, its type while nothing uses it, its name, how much a chat
// model reasons before answering, its provider, what a minute of audio costs a
// transcription model and, for chat, extra fields under ADVANCED. Which model each thing uses is picked in the tab that owns it; here it
// is only read. Renaming a model renames it wherever it is used; one in use as the
// assistant or the default transcription cannot be removed, and removing another
// first says what falls back. A new model starts from the provider of the last one
// of its type, its name taking the keyboard.
ColumnLayout {
  id: editor

  required property var draft
  // Provider presets by model type: {chat: [...], transcription: [...]}.
  required property var presets
  // Applies a change to a copy of the draft.
  signal changed(var change)
  // Counts the window's discards: each brings back the extra fields saved.
  property int resets: 0
  // Messages by field path ("models.<index>.name", ".base_url", ".model"), as
  // the settings window shows them.
  property var errors: ({})
  // Some card's extra fields being typed are not valid JSON.
  signal invalidExtra(bool invalid)
  property int invalidCards: 0
  onInvalidCardsChanged: invalidExtra(invalidCards > 0)

  // The index of the model shown open, or -1.
  property int open: -1
  // The index of the model just added, whose name takes the keyboard once its card is made, or -1.
  property int naming: -1
  readonly property var models: draft.models || []
  spacing: 8

  // Opens the card of the field at `path` and gives that field the keyboard;
  // "models.extra" is the first card whose extra fields are not valid JSON.
  function reveal(path) {
    const parts = path.split(".")
    if (parts[0] !== "models")
      return
    const at = parts[1] === "extra" ? [...Array(cards.count).keys()].find(i => cards.itemAt(i).extraInvalid) : Number(parts[1])
    if (at === undefined)
      return
    editor.open = at
    Qt.callLater(() => cards.itemAt(at).focusField(parts[1] === "extra" ? "extra" : parts[2]))
  }

  // The provider-list target of a model type.
  function target(type) { return type === "chat" ? "llm" : "stt" }

  // What answers with the model `entry` now, as `answering` picks it, as labels:
  // the defaults, the reviewer, the skills that name it and each session kind's uses.
  function users(entry) {
    const found = []
    const answers = (use, kind, action) => Draft.answering(draft, use, kind, action).name === entry.name
    const uses = entry.type === "chat" ? ["chat", "translation"] : ["transcription"]
    const label = (use, kind) => use === "translation" ? I18n.t("settings.translates_kind", { kind: Eco.kindName(kind).toUpperCase() }) : Eco.kindName(kind).toUpperCase()
    if (entry.type === "chat" && answers("chat", "", null))
      found.push(I18n.t("settings.role_assistant"))
    if (entry.type === "transcription" && answers("transcription", "", null))
      found.push(I18n.t("settings.role_transcriber"))
    if (entry.type === "chat" && answers("translation", "", null))
      found.push(I18n.t("settings.translation"))
    if (entry.type === "chat" && draft.reviewer && draft.reviewer.enabled && draft.reviewer.model === entry.name)
      found.push(I18n.t("settings.role_reviewer"))
    draft.actions.forEach(action => { if (action.model === entry.name) found.push(action.name) })
    draft.kinds.forEach(kind => uses.forEach(use => { if (answers(use, kind, null)) found.push(label(use, kind)) }))
    return found
  }

  Repeater {
    id: cards
    model: editor.models.length
    delegate: SettingsCard {
      id: card
      required property int index
      // Past the list for a moment while a removal shrinks it.
      readonly property var entry: editor.models[index] || ({ name: "", type: "chat", model: "" })
      readonly property bool chat: entry.type === "chat"
      readonly property var used: editor.users(entry)
      property bool advanced: false
      readonly property string savedExtra: JSON.stringify(entry.extra || {})
      // The text in the extra field, and whether it is not valid JSON.
      property string extraText: savedExtra
      property bool extraInvalid: false
      onExtraInvalidChanged: editor.invalidCards += extraInvalid ? 1 : -1
      Component.onDestruction: if (extraInvalid) editor.invalidCards -= 1
      function reload() {
        extraText = savedExtra
        extraInvalid = false
      }
      // The draft's extra fields changed other than by typing here: the field shows them.
      onSavedExtraChanged: {
        let typed = null
        try { typed = JSON.stringify(JSON.parse(extraText)) } catch (error) {}
        if (typed !== savedExtra)
          reload()
      }
      // The price per minute as typed, so "0.0" stays while its digits follow.
      readonly property string savedPrice: entry.price_per_minute === undefined ? "" : String(entry.price_per_minute)
      property string priceText: savedPrice
      onSavedPriceChanged: if (priceText === "" ? savedPrice !== "" : Number(priceText.replace(",", ".")) !== Number(savedPrice)) priceText = savedPrice
      Connections {
        target: editor
        function onResetsChanged() { card.reload() }
      }
      // Once laid out, so the page scrolls to where the field really is.
      Component.onCompleted: if (editor.naming === index) {
        editor.naming = -1
        Qt.callLater(() => nameField.input.forceActiveFocus())
      }

      function change(key, value) {
        editor.changed(d => {
          if (value === null)
            delete d.models[index][key]
          else
            d.models[index][key] = value
        })
      }
      function rename(name) {
        const old = entry.name
        editor.changed(d => {
          d.models[index].name = name
          if (d.llm.model === old) d.llm.model = name
          if (d.stt.model === old) d.stt.model = name
          if (d.reviewer && d.reviewer.model === old) d.reviewer.model = name
          if (d.translation && d.translation.model === old) d.translation.model = name
          d.actions.forEach(action => { if (action.model === old) action.model = name })
        })
      }
      // Gives the keyboard to the field of `key`: "name", "base_url", "model" or "extra".
      function focusField(key) {
        if (key === "name")
          nameField.input.forceActiveFocus()
        else if (key === "extra") {
          advanced = true
          Qt.callLater(() => extraField.input.forceActiveFocus())
        } else
          provider.focusField(key)
      }
      // The prefix of its fields' paths in the window's errors.
      readonly property string path: "models." + index + "."
      // Make an unused model the other type, keeping its name and model id.
      function retype(type) {
        Eco.clearModels(editor.target(type))
        editor.changed(d => Draft.retype(d, index, type, editor.presets))
      }

      title: entry.name
      placeholder: I18n.t("settings.model_number", { number: index + 1 })
      note: [I18n.t(chat ? "settings.type_chat" : "settings.type_transcription"), entry.model,
        used.length ? I18n.t("settings.used_by", { users: used.join(", ") }) : ""].filter(part => part).join("  ·  ")
      expanded: editor.open === index
      removable: editor.draft.llm.model !== entry.name && editor.draft.stt.model !== entry.name
      removeTip: I18n.t("settings.remove_model")
      onToggled: {
        editor.open = expanded ? -1 : index
        Eco.clearModels(editor.target(entry.type))
      }
      onRemoved: remove.ask(index)

      Label {
        Layout.fillWidth: true
        visible: card.used.length > 0
        text: I18n.t("settings.used_by", { users: card.used.join(", ") })
        color: Theme.dim
        font.pixelSize: 11
        wrapMode: Text.Wrap
      }

      ColumnLayout {
        visible: card.used.length === 0
        spacing: 6
        Caption { text: I18n.t("settings.model_type") }
        Flow {
          spacing: 6
          Repeater {
            model: ["chat", "transcription"]
            delegate: Chip {
              required property string modelData
              text: modelData === "chat" ? I18n.t("settings.type_chat") : I18n.t("settings.type_transcription")
              checked: card.entry.type === modelData
              onClicked: if (!checked) card.retype(modelData)
            }
          }
        }
      }

      Field {
        id: nameField
        Layout.fillWidth: true
        label: I18n.t("settings.model_name")
        value: card.entry.name
        placeholder: card.placeholder
        error: editor.errors[card.path + "name"] || ""
        onEdited: text => card.rename(text)
      }

      ColumnLayout {
        visible: card.chat
        spacing: 6
        Caption { text: I18n.t("settings.reasoning") }
        Flow {
          spacing: 6
          Repeater {
            // "" leaves it to the provider, or to a `reasoning` in the extra fields;
            // the rest is sent with every request.
            model: ["", "off", "low", "medium", "high"]
            delegate: Chip {
              required property string modelData
              text: ({ "": I18n.t(card.entry.extra && card.entry.extra.reasoning ? "settings.reasoning_extra" : "settings.reasoning_provider"), off: I18n.t("settings.reasoning_off"), low: I18n.t("settings.reasoning_low"),
                       medium: I18n.t("settings.reasoning_medium"), high: I18n.t("settings.reasoning_high") })[modelData]
              tip: modelData === "off" ? I18n.t("settings.reasoning_off_tip") : ""
              checked: (card.entry.reasoning || "") === modelData
              onClicked: card.change("reasoning", modelData || null)
            }
          }
        }
      }

      ProviderFields {
        id: provider
        Layout.fillWidth: true
        target: editor.target(card.entry.type)
        errors: ({ base_url: editor.errors[card.path + "base_url"], model: editor.errors[card.path + "model"] })
        section: card.entry
        presets: editor.presets[card.entry.type]
        onEdited: (key, value) => {
          card.change(key, value)
          if (key === "extra")
            card.reload()
        }
      }

      Field {
        Layout.fillWidth: true
        visible: !card.chat
        label: I18n.t("settings.price_per_minute")
        placeholder: I18n.t("settings.price_per_minute_unset")
        value: card.priceText
        input.validator: RegularExpressionValidator { regularExpression: /[0-9]{0,6}([.,][0-9]{0,6})?/ }
        input.inputMethodHints: Qt.ImhFormattedNumbersOnly
        onEdited: text => {
          card.priceText = text
          const price = Number(text.replace(",", "."))
          if (text === "")
            card.change("price_per_minute", null)
          else if (Number.isFinite(price))
            card.change("price_per_minute", price)
        }
      }

      Chip {
        visible: card.chat
        text: I18n.t("settings.advanced")
        icon: card.advanced ? "chevron-down" : "chevron-right"
        checked: card.advanced
        onClicked: card.advanced = !card.advanced
      }

      Field {
        id: extraField
        Layout.fillWidth: true
        visible: card.chat && card.advanced
        label: I18n.t("config.extra")
        value: card.extraText
        error: card.extraInvalid ? I18n.t("settings.invalid_extra") : ""
        onEdited: text => {
          card.extraText = text
          try {
            const extra = JSON.parse(text)
            if (!extra || typeof extra !== "object" || Array.isArray(extra))
              throw new Error("invalid extra")
            card.extraInvalid = false
            card.change("extra", extra)
          } catch (error) {
            card.extraInvalid = true
          }
        }
      }
    }
  }

  ConfirmDialog {
    id: remove
    property int index: -1
    property string name: ""
    property var consequences: []
    // Asks before removing the model at `index`, saying what falls back and what turns off.
    function ask(index) {
      const entry = editor.models[index]
      const change = Draft.removal(editor.draft, index)
      const groups = {}
      change.moves.forEach(move => {
        const label = move.action ? move.action
          : move.use !== "translation" ? Eco.kindName(move.kind).toUpperCase()
          : move.kind ? I18n.t("settings.translates_kind", { kind: Eco.kindName(move.kind).toUpperCase() })
          : I18n.t("settings.translation")
        ;(groups[move.to] = groups[move.to] || []).push(label)
      })
      remove.index = index
      remove.name = entry.name || I18n.t("settings.model_number", { number: index + 1 })
      remove.consequences = Object.keys(groups).map(to => I18n.t(to ? "settings.remove_falls_back" : "settings.remove_no_model",
        { users: groups[to].join(", "), model: to })).concat(change.reviewerOff ? [I18n.t("settings.remove_reviewer_off")] : [])
      remove.open()
    }
    title: I18n.t("settings.remove_model_title")
    question: I18n.t("settings.remove_model_question", { name: name })
    warning: consequences.join("\n")
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: {
      const index = remove.index
      editor.open = -1
      editor.changed(d => Draft.removeModel(d, index))
    }
  }

  Chip {
    text: I18n.t("settings.add_model")
    icon: "plus"
    onClicked: {
      editor.open = editor.models.length
      editor.naming = editor.models.length
      Eco.clearModels("llm")
      editor.changed(d => d.models = [...(d.models || []), Object.assign({ name: "", type: "chat", model: "" }, Draft.provider(d, "chat", editor.presets))])
    }
  }
}
