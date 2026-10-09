pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import "draft.js" as Draft

// KindSettings lists the session kinds as cards: closed, the kind, whether it is
// the default and the models its sessions run on; open, its name, a chip that
// makes it the default (the first, suggested when a session is created or
// imported) and its own transcription, assistant and translation models, or the
// defaults.
// A kind is renamed once its name is left or Enter is pressed, never to an empty
// or another kind's name; renaming or removing it follows it into the models and
// context slots, and removing it asks first.
ColumnLayout {
  id: editor

  required property var draft
  // Messages by field path ("kinds.<index>"), as the settings window shows them.
  property var errors: ({})
  // Applies a change to a copy of the draft.
  signal changed(var change)
  // The index of the kind shown open, or -1.
  property int open: -1
  readonly property var kinds: draft.kinds
  spacing: 8

  // Replace `kind` by `next` (or drop it, for null) in every model and context slot.
  function follow(d, kind, next) {
    const swap = list => list.map(k => k === kind ? next : k).filter(k => k !== null)
    d.models.forEach(model => {
      for (const field of ["kinds", "translates"]) {
        if (!model[field])
          continue
        model[field] = swap(model[field])
        if (!model[field].length)
          delete model[field]
      }
    })
    ;(d.contexts || []).forEach(slot => slot.kinds = swap(slot.kinds))
  }

  // Applies the names being typed.
  function commit() {
    for (let i = 0; i < cards.count; i++)
      cards.itemAt(i).commit()
  }

  // Opens the card of the kind at `path` and gives its name the keyboard.
  function reveal(path) {
    const [list, at] = path.split(".")
    if (list !== "kinds")
      return
    editor.open = Number(at)
    Qt.callLater(() => cards.itemAt(Number(at)).nameField.input.forceActiveFocus())
  }

  Repeater {
    id: cards
    model: editor.kinds.length
    delegate: SettingsCard {
      id: card
      required property int index
      // Past the list for a moment while a removal shrinks it.
      readonly property string kind: editor.kinds[index] ?? ""
      readonly property alias nameField: nameField
      // The name as typed, applied by `commit`.
      property string typed: kind
      onKindChanged: typed = kind
      readonly property string name: typed.trim()
      readonly property string typedError: name === "" ? (kind ? I18n.t("settings.error_empty") : "")
        : editor.kinds.some((other, i) => i !== index && other === name) ? I18n.t("settings.error_taken") : ""

      // Renames the kind to the name typed; an empty or taken one brings its name back.
      function commit() {
        if (name === kind)
          return
        if (typedError !== "" || name === "") {
          typed = kind
          nameField.input.text = kind
          return
        }
        const from = kind
        const to = name
        editor.changed(d => {
          d.kinds[card.index] = to
          editor.follow(d, from, to)
        })
      }

      title: kind ? Eco.kindName(kind) : ""
      placeholder: I18n.t("settings.kind_number", { number: index + 1 })
      note: [index === 0 ? I18n.t("settings.kind_default") : "",
        I18n.t("settings.kind_note", { transcription: Draft.answering(editor.draft, "transcription", kind, null).name, chat: Draft.answering(editor.draft, "chat", kind, null).name })].filter(part => part).join("  ·  ")
      expanded: editor.open === index
      removable: editor.kinds.length > 1
      removeTip: I18n.t("settings.remove_kind")
      onToggled: editor.open = expanded ? -1 : index
      onRemoved: remove.ask(index)

      Field {
        id: nameField
        Layout.fillWidth: true
        label: I18n.t("settings.kind_name")
        value: card.kind
        // The window's error is on the name applied, not on one still being typed.
        error: card.typedError || (card.name === card.kind ? editor.errors["kinds." + card.index] || "" : "")
        onEdited: text => card.typed = text
        onCommitted: card.commit()
      }
      Chip {
        text: I18n.t(card.index === 0 ? "settings.is_default" : "settings.default")
        checked: card.index === 0
        onClicked: if (card.index > 0) {
          editor.open = 0
          editor.changed(d => d.kinds.unshift(d.kinds.splice(card.index, 1)[0]))
        }
      }
      KindModels {
        Layout.fillWidth: true
        draft: editor.draft
        kind: card.kind
        onChanged: change => editor.changed(change)
      }
      Label { Layout.fillWidth: true; text: I18n.t("settings.kind_chat_note"); color: Theme.dim; font.pixelSize: 11; wrapMode: Text.Wrap }
    }
  }

  ConfirmDialog {
    id: remove
    property int index: -1
    property string name: ""
    property var consequences: []
    // Asks before removing the kind at `index`, saying that its sessions keep
    // it, which kind becomes the default and that its own models and context
    // slots stop applying to it.
    function ask(index) {
      const kind = editor.kinds[index]
      const own = editor.draft.models.some(model => (model.kinds || []).includes(kind) || (model.translates || []).includes(kind))
        || (editor.draft.contexts || []).some(slot => slot.kinds.includes(kind))
      remove.index = index
      remove.name = kind ? Eco.kindName(kind) : I18n.t("settings.kind_number", { number: index + 1 })
      remove.consequences = [kind ? I18n.t("settings.remove_kind_sessions") : "",
        index === 0 ? I18n.t("settings.remove_kind_default", { name: Eco.kindName(editor.kinds[1]) }) : "",
        own ? I18n.t("settings.remove_kind_models") : ""].filter(line => line)
      remove.open()
    }
    title: I18n.t("settings.remove_kind_title")
    question: I18n.t("settings.remove_kind_question", { name: name })
    warning: consequences.join("\n")
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: {
      const index = remove.index
      const kind = editor.kinds[index]
      editor.open = -1
      editor.changed(d => {
        d.kinds.splice(index, 1)
        editor.follow(d, kind, null)
      })
    }
  }

  Chip {
    text: I18n.t("settings.add_kind")
    icon: "plus"
    onClicked: {
      editor.open = editor.kinds.length
      editor.changed(d => d.kinds.push(""))
    }
  }
}
