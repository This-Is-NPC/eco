pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic as C
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ComposerSurface asks, runs a skill or keeps a note: the skills as numbered
// chips, and a box where `/` lists them as you type. The box grows with what
// is typed, up to `maxEntryHeight`, and scrolls past it. Where nothing answers a
// question, what is typed is kept as a note, and SEND and the skills nothing
// answers stay, off, saying why.
ColumnLayout {
  id: surface

  property var actions: []
  // The actions that run here; the others show off, with `reason`.
  property var ready: actions
  // Why a question or a skill not ready cannot run here.
  property string reason
  property string selectedAction: ""
  property string placeholder
  property bool compact: false
  property bool busy: false
  // Whether a question typed is asked; otherwise Enter keeps it as a note.
  property bool asks: true
  // Whether the actions show as chips above the box; `/` reaches them either way.
  property bool actionsShown: true
  // The `/name` sent that names no action; the box keeps it to be fixed.
  property string unknownAction: ""
  // The tallest the box grows before what is typed scrolls in it.
  property real maxEntryHeight: 180

  signal actionChosen(string action)
  signal submitted(string text)
  // The text is kept as a note: context for later answers, not a question.
  signal noted(string text)

  spacing: 10

  // Put the keyboard in the box.
  function focusEntry() { entry.forceActiveFocus() }

  // The text typed, trimmed; the box is emptied once there is any.
  function take() {
    const value = entry.text.trim()
    if (value)
      entry.text = ""
    unknownAction = ""
    return value
  }

  // Whether the text changes from the code, not from typing.
  property bool filling: false
  function fill(value) {
    filling = true
    entry.text = value
    entry.cursorPosition = value.length
    filling = false
  }

  // Runs a skill from the box, which it empties.
  function run(name) {
    completion.close()
    entry.text = ""
    unknownAction = ""
    if (surface.busy)
      sendButton.restartTrace()
    actionChosen(name)
  }

  // A lone `/word` runs that skill and is never asked; one that names no skill stays in the box.
  function submit() {
    const slash = entry.text.trim().match(/^\/([^\s\/]+)$/)
    if (slash && !actions.includes(slash[1])) {
      unknownAction = slash[1]
      return
    }
    if (slash) {
      run(slash[1])
      return
    }
    if (!asks) {
      note()
      return
    }
    const value = take()
    if (!value)
      return
    if (surface.busy)
      sendButton.restartTrace()
    submitted(value)
  }

  function note() {
    completion.close()
    const value = take()
    if (value)
      noted(value)
  }

  // The skills a lone `/word` being typed may name: those starting with it first.
  readonly property var matches: {
    const typed = entry.text.match(/^\/([^\s\/]*)$/)
    if (!typed)
      return []
    const word = typed[1].toLowerCase()
    const named = actions.filter(name => name.toLowerCase().includes(word))
    return named.filter(name => name.toLowerCase().startsWith(word))
      .concat(named.filter(name => !name.toLowerCase().startsWith(word)))
  }
  // Esc closed the list; typing opens it again.
  property bool dismissed: false

  function suggest() {
    if (matches.length === 0 || dismissed || !entry.activeFocus) {
      completion.close()
      return
    }
    completion.move(0)
    completion.open()
  }
  onMatchesChanged: suggest()
  onDismissedChanged: suggest()

  Flow {
    Layout.fillWidth: true
    visible: surface.actionsShown && surface.actions.length > 0
    spacing: 6
    Repeater {
      model: surface.actions
      delegate: Chip {
        required property string modelData
        required property int index
        // Numbered as Alt and the number run it.
        text: (index < 9 ? (index + 1) + "  " : "") + modelData.toUpperCase()
        tip: index < 9 ? "Alt+" + (index + 1) : ""
        reason: surface.ready.includes(modelData) ? "" : surface.reason
        checked: surface.selectedAction === modelData
        onClicked: surface.actionChosen(modelData)
      }
    }
  }

  Rectangle {
    id: box
    Layout.fillWidth: true
    implicitHeight: Math.max(42, Math.min(entry.implicitHeight, surface.maxEntryHeight) + 4)
    color: Theme.highlight
    border.color: surface.unknownAction ? Theme.error : entry.activeFocus ? Theme.primary : Theme.line
    border.width: entry.activeFocus || surface.unknownAction ? 2 : 1
    Behavior on border.color { ColorAnimation { duration: 140 } }

    // The prompt sits by the first line and the buttons by the last.
    RowLayout {
      anchors { fill: parent; leftMargin: 14; rightMargin: 8; topMargin: 2; bottomMargin: 2 }
      spacing: 10
      Icon { Layout.alignment: Qt.AlignTop; Layout.topMargin: 12; name: "chevron-right"; size: 14; color: entry.activeFocus ? Theme.primary : Theme.dim }
      Flickable {
        id: scroller
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        boundsBehavior: Flickable.StopAtBounds
        C.TextArea.flickable: TextBlock {
          id: entry
          submits: false
          tabTaken: completion.visible
          placeholderText: surface.placeholder
          font.pixelSize: 13
          leftPadding: 0
          rightPadding: 0
          topPadding: 10
          bottomPadding: 10
          background: Item {}
          onTextChanged: if (!surface.filling) {
            surface.unknownAction = ""
            surface.dismissed = false
          }
          // Back in the box, the list a `/word` names shows again.
          onActiveFocusChanged: surface.suggest()
          // The open list takes ↑/↓, Tab completes, Enter runs, Esc closes it.
          // Enter sends, Shift+Enter breaks the line, Ctrl+Enter keeps the
          // text as a note.
          Keys.onPressed: event => {
            const enter = event.key === Qt.Key_Return || event.key === Qt.Key_Enter
            const shift = event.modifiers & Qt.ShiftModifier
            if (completion.visible && (event.key === Qt.Key_Down || event.key === Qt.Key_Up))
              completion.move(completion.current + (event.key === Qt.Key_Down ? 1 : -1))
            else if (completion.visible && event.key === Qt.Key_Tab) {
              surface.dismissed = true
              surface.fill("/" + surface.matches[completion.current])
            } else if (completion.visible && enter && !shift)
              completion.pick(completion.current)
            else if (completion.visible && event.key === Qt.Key_Escape)
              surface.dismissed = true
            else if (enter && (event.modifiers & Qt.ControlModifier))
              surface.note()
            else if (enter && !shift)
              surface.submit()
            else
              return
            event.accepted = true
          }
        }
      }
      TraceButton {
        Layout.alignment: Qt.AlignBottom
        Layout.bottomMargin: 5
        dense: true
        role: "progress"
        implicitWidth: surface.compact ? 32 : 110
        icon: "plus"
        text: surface.compact ? "" : I18n.t("composer.note")
        tip: I18n.t("composer.note_tip")
        onClicked: surface.note()
      }
      TraceButton {
        id: sendButton
        Layout.alignment: Qt.AlignBottom
        Layout.bottomMargin: 5
        dense: true
        role: "progress"
        implicitWidth: surface.compact ? 32 : 86
        busy: surface.busy
        allowClickWhileBusy: true
        icon: "enter"
        text: surface.compact ? "" : I18n.t("composer.send")
        tip: I18n.t("composer.send")
        reason: surface.asks ? "" : surface.reason
        onClicked: surface.submit()
      }
    }

    MenuPopup {
      id: completion
      above: true
      takesKeyboard: false
      minimumWidth: 160
      entries: surface.matches.map(name => ({ text: "/" + name }))
      onPicked: index => surface.run(surface.matches[index])
    }

    // Alt and a skill's number run it, its chip shown or not.
    Repeater {
      model: Math.min(9, surface.actions.length)
      delegate: Item {
        id: key
        required property int index
        Shortcut {
          sequence: "Alt+" + (key.index + 1)
          enabled: surface.visible && surface.enabled && surface.ready.includes(surface.actions[key.index])
          onActivated: surface.actionChosen(surface.actions[key.index])
        }
      }
    }
  }

  Label {
    Layout.fillWidth: true
    visible: surface.unknownAction !== ""
    text: I18n.t("composer.unknown_action", { name: surface.unknownAction })
    color: Theme.error
    font.pixelSize: 10
    wrapMode: Text.Wrap
  }
}
