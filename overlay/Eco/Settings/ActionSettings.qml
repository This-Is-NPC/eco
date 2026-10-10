pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ActionSettings lists the actions as cards, in the order the composer offers
// them: each shows its name and the model it runs on closed, and opens to edit
// its name, prompt, output format, model and hook: a command run with the
// session it answered for, on its own or when sent. Cards move up and down;
// removing one asks first. A new action opens ready to fill in.
ColumnLayout {
  id: editor
  required property var actions
  // The settings draft, for the models an action can run on.
  required property var draft
  // The names the actions are saved with, which shortcuts may call.
  property var savedNames: []
  // Messages by field path ("actions.<index>"), as the settings window shows them.
  property var errors: ({})
  signal changed(var actions)
  // The index of the action shown open, or -1.
  property int open: -1
  spacing: 8

  // A saved name no action has any more: some action was renamed or removed.
  readonly property bool nameLost: savedNames.some(name => !actions.some(action => action.name.trim() === name))

  // Opens the card of the action at `path` and gives its name the keyboard.
  function reveal(path) {
    const [list, at] = path.split(".")
    if (list !== "actions")
      return
    editor.open = Number(at)
    Qt.callLater(() => cards.itemAt(Number(at)).nameField.input.forceActiveFocus())
  }

  Repeater {
    id: cards
    model: editor.actions.length
    delegate: SettingsCard {
      id: card
      required property int index
      readonly property var action: editor.actions[index] || ({ name: "", prompt: "", format: "" })
      readonly property var answer: Draft.answering(editor.draft, "chat", "", action)
      readonly property alias nameField: nameField

      function change(key, value) {
        const next = JSON.parse(JSON.stringify(editor.actions))
        if ((key === "model" || key === "hook" || key === "hook_auto") && !value)
          delete next[index][key]
        else
          next[index][key] = value
        // Without a hook there is nothing to send on its own.
        if (!next[index].hook)
          delete next[index].hook_auto
        editor.changed(next)
      }

      title: action.name
      placeholder: I18n.t("settings.action_number", { number: index + 1 })
      note: [answer.own ? answer.name : I18n.t("settings.default_model", { name: answer.name }), action.hook ? I18n.t(action.hook_auto ? "settings.action_hook_on" : "settings.action_hook_send") : ""].filter(part => part).join("  ·  ")
      expanded: editor.open === index
      removeTip: I18n.t("settings.remove_action")
      movable: true
      first: index === 0
      last: index === editor.actions.length - 1
      onToggled: editor.open = expanded ? -1 : index
      onRemoved: remove.ask(index)
      // The card goes with its action, open as it was, keeping the keyboard.
      onMoved: step => {
        const to = index + step
        const next = editor.actions.slice()
        next.splice(to, 0, next.splice(index, 1)[0])
        if (editor.open === index)
          editor.open = to
        else if (editor.open === to)
          editor.open = index
        editor.changed(next)
        cards.itemAt(to).takeFocus()
      }

      Field {
        id: nameField
        Layout.fillWidth: true
        label: I18n.t("settings.action_name")
        value: card.action.name
        error: editor.errors["actions." + card.index] || ""
        onEdited: text => card.change("name", text)
      }
      Label {
        Layout.fillWidth: true
        visible: editor.nameLost && card.action.name.trim() !== "" && !editor.savedNames.includes(card.action.name.trim())
        text: I18n.t("settings.action_renamed")
        color: Theme.warning
        font.pixelSize: 11
        wrapMode: Text.Wrap
      }
      SettingsTextArea { Layout.fillWidth: true; label: I18n.t("settings.action_prompt"); value: card.action.prompt; onEdited: text => card.change("prompt", text) }
      SettingsTextArea { Layout.fillWidth: true; label: I18n.t("settings.action_format"); value: card.action.format; onEdited: text => card.change("format", text) }
      ModelPick {
        Layout.fillWidth: true
        draft: editor.draft
        skill: card.index
        text: I18n.t("settings.action_model")
        onPicked: name => card.change("model", name)
      }
      Field { Layout.fillWidth: true; label: I18n.t("settings.action_hook"); value: card.action.hook || ""; onEdited: text => card.change("hook", text) }
      Label { Layout.fillWidth: true; text: I18n.t("settings.action_hook_note"); color: Theme.dim; font.pixelSize: 11; wrapMode: Text.Wrap }
      Chip {
        enabled: !!card.action.hook
        text: I18n.t("settings.action_hook_auto")
        tip: I18n.t("settings.action_hook_auto_tip")
        checked: !!card.action.hook_auto
        onClicked: card.change("hook_auto", !card.action.hook_auto)
      }
    }
  }

  ConfirmDialog {
    id: remove
    property int index: -1
    property string name: ""
    property var consequences: []
    // Asks before removing the action at `index`, saying that shortcuts calling
    // it stop working and that the actions after it move up.
    function ask(index) {
      const name = editor.actions[index].name.trim()
      remove.index = index
      remove.name = name || I18n.t("settings.action_number", { number: index + 1 })
      remove.consequences = [name ? I18n.t("settings.remove_action_shortcut", { name: name }) : "",
        index < editor.actions.length - 1 ? I18n.t("settings.remove_action_order") : ""].filter(line => line)
      remove.open()
    }
    title: I18n.t("settings.remove_action_title")
    question: I18n.t("settings.remove_action_question", { name: name })
    warning: consequences.join("\n")
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: {
      const next = editor.actions.slice()
      next.splice(remove.index, 1)
      editor.open = -1
      editor.changed(next)
    }
  }

  Chip {
    text: I18n.t("settings.add_action")
    icon: "plus"
    onClicked: {
      editor.open = editor.actions.length
      editor.changed([...editor.actions, { name: "", prompt: "", format: "" }])
    }
  }
}
