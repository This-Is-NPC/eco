pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// PeopleView lists the people eco keeps in the DataTable: open their sessions,
// add someone, colour, rename, merge two who are the same person — confirmed,
// naming who stays — or forget one. Below 420 px a row's tools fold into one
// menu, so the name keeps the room.
// Keys: ↑/↓ choose, Enter opens, F2 renames, M merges, Esc cancels a merge or goes back.
ColumnLayout {
  id: people
  spacing: 14

  property bool active: false
  // The person whose voices are being merged into another, or null.
  property var merging: null
  readonly property bool folded: width < 420
  onActiveChanged: merging = null
  // Whether the last person picked to merge with was the one merging.
  property bool pickedSelf: false
  onMergingChanged: pickedSelf = false

  Keys.onEscapePressed: back()
  Keys.onPressed: event => {
    const person = table.currentEntry
    if (person === null || merging !== null)
      return
    if (event.key === Qt.Key_F2)
      rename(person)
    else if (event.key === Qt.Key_M && Eco.people.length > 1)
      merging = person
    else
      return
    event.accepted = true
  }

  function back() {
    if (merging !== null)
      merging = null
    else
      Eco.goBack()
  }

  function add() { Eco.renameRequested({ newPerson: true }) }
  function rename(person) { Eco.renameRequested({ personId: person.id, name: person.name }) }
  function forgetting(person) {
    forget.person = person
    forget.open()
  }
  // The second person picked while merging: ask before the two become one.
  function pick(person) {
    if (person.id === merging.id) {
      pickedSelf = true
      nudge.restart()
      return
    }
    merge.from = merging
    merge.into = person
    merge.open()
  }

  RowLayout {
    Layout.fillWidth: true
    spacing: 10
    TraceButton {
      id: backButton
      dense: true
      role: "navigation"
      // While merging, the notice's CANCEL is the one way out (and Esc).
      visible: people.merging === null
      icon: "chevron-left"
      text: Eco.backText
      tip: "esc"
      onClicked: people.back()
    }
    Item { Layout.fillWidth: true }
    Label { text: I18n.t("people.count", { n: Eco.people.length }); color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 3 }
    TraceButton {
      visible: people.merging === null
      dense: true
      role: "action"
      icon: "plus"
      text: I18n.t("people.add")
      onClicked: people.add()
    }
  }

  StatusMessage {
    id: banner
    Layout.fillWidth: true
    visible: people.merging !== null
    variant: "toast"
    kind: "info"
    message: people.merging ? I18n.t(people.pickedSelf ? "people.merging_self" : "people.merging", { name: people.merging.name }) : ""
    Chip { text: I18n.t("dialog.cancel"); onClicked: people.merging = null }
  }
  GuardNudge { id: nudge; targetObject: banner }

  DataTable {
    id: table
    Layout.fillWidth: true
    Layout.fillHeight: true
    active: people.active
    rows: Eco.people
    columns: [
      { title: I18n.t("people.column.name"), weight: 0.5 },
      { title: I18n.t("people.column.voices"), weight: 0.25 },
      { title: I18n.t("people.column.sessions"), weight: 0.25 }
    ]
    cells: person => [person.name, I18n.t("people.voices", { n: person.voices }), I18n.t("people.sessions", { n: person.sessions.length })]
    tone: (person, column) => column === 0 ? Eco.personColor(person.id) : ""
    stacked: [[0], [1, 2]]
    emptyText: I18n.t("people.empty")
    emptyAction: I18n.t("people.add")
    emptyIcon: "plus"
    onEmptyActed: people.add()
    dimmed: person => people.merging !== null && people.merging.id === person.id
    toolsFolded: people.folded
    toolsWidth: people.folded ? 28 : 166
    tools: personTools
    onOpened: person => people.merging === null ? Eco.historyForPerson(person.id) : people.pick(person)
    onRemoveRequested: person => {
      if (people.merging === null)
        people.forgetting(person)
    }
    onBackRequested: backButton.press()
  }

  // A row's tools; folded, one menu holds them and still opens the colour grid.
  Component {
    id: personTools
    Row {
      id: tools
      property var entry
      spacing: 10
      enabled: people.merging === null
      ColorPicker {
        id: picker
        visible: !people.folded
        anchors.verticalCenter: parent.verticalCenter
        chosen: tools.entry ? tools.entry.color || "" : ""
        automatic: tools.entry ? Eco.personColor(tools.entry.id) : Theme.dim
        onPicked: color => Eco.setPersonColor(tools.entry.id, color)
      }
      IconButton {
        visible: !people.folded
        name: "edit"
        tip: I18n.t("people.rename_tip")
        onClicked: people.rename(tools.entry)
      }
      IconButton {
        visible: !people.folded && Eco.people.length > 1
        name: "merge"
        tip: I18n.t("people.merge_tip")
        onClicked: people.merging = tools.entry
      }
      IconButton {
        visible: !people.folded
        name: "close"
        tone: Theme.error
        tip: I18n.t("people.forget_tip")
        onClicked: people.forgetting(tools.entry)
      }
      ActionMenu {
        visible: people.folded
        tip: I18n.t("detail.more")
        items: [
          { id: "color", icon: "color", text: I18n.t("people.color") },
          { id: "rename", icon: "edit", text: I18n.t("people.rename") }
        ].concat(Eco.people.length > 1 ? [{ id: "merge", icon: "merge", text: I18n.t("people.merge") }] : [])
          .concat([{ id: "forget", icon: "close", text: I18n.t("people.forget"), guarded: true }])
        onChosen: id => {
          if (id === "color")
            picker.open()
          else if (id === "rename")
            people.rename(tools.entry)
          else if (id === "merge")
            people.merging = tools.entry
          else
            people.forgetting(tools.entry)
        }
      }
    }
  }

  // Which way the two merge is the user's: SWAP turns it around.
  ConfirmDialog {
    id: merge
    property var from: null
    property var into: null
    title: I18n.t("people.merge_title")
    question: from && into ? I18n.t("people.merge_question", { from: from.name, into: into.name }) : ""
    confirmIcon: "merge"
    confirmText: I18n.t("people.merge")
    onConfirmed: {
      Eco.mergePeople(into.id, from.id)
      people.merging = null
    }
    Chip {
      text: I18n.t("people.merge_swap")
      onClicked: [merge.from, merge.into] = [merge.into, merge.from]
    }
  }

  ConfirmDialog {
    id: forget
    property var person: null
    title: I18n.t("people.forget_title")
    question: I18n.t("people.forget_question", { name: person ? person.name : "" })
    warning: person ? [
      person.voices > 0 ? I18n.t("people.forget_voices", { n: person.voices, name: person.name }) : I18n.t("people.forget_no_voice", { name: person.name }),
      person.sessions.length > 0 ? I18n.t("people.forget_sessions", { n: person.sessions.length, name: person.name }) : ""
    ].filter(text => text).join(" ") : ""
    confirmText: I18n.t("people.forget")
    onConfirmed: Eco.forgetPerson(person.id)
  }
}
