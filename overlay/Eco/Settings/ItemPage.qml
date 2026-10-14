pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ItemPage is the screen of one item of the settings: what kind it is, its
// name and how far its edit is; read, what it is (`facts`) beside what is
// `related` to it, when anything is, each related item a link that opens where
// it is set, then its long `details` across the screen, and the collections it
// holds, each a child table;
// edited, its form, which holds no collection. Its commands, for the bar, go
// back, edit and delete it, add to its child tables and change the child row
// picked, which saves at once, or, editing, save, discard and cancel, then its
// own `extra`. A child table adds through a child screen opened over the
// item, and a child row may open one of its own. The item of a section that is
// only that item is never deleted.
Item {
  id: page

  // What kind of item it is, as read: "model", "skill", ...; "" for the item of
  // a section that is only that item, which the section names.
  property string kind: ""
  property string title: ""
  // Shown in the title's place while it has none, as a model not named yet.
  property string placeholder: ""
  // The edit; an item only read has none.
  property ItemDraft draft: null
  // The config it shows: the draft while edited, else the saved one.
  readonly property var config: draft !== null && draft.active ? draft.draft : Eco.config
  property bool editing: false
  // The section is this item alone: it is never deleted, and it has a way
  // back only when it was followed to.
  property bool single: false
  property bool backable: !single
  // A part of the config that is never removed: it has no DELETE.
  property bool fixed: false
  // Whether DELETE may remove it, and why not when it may not.
  property bool removable: true
  property string keptReason: ""
  // [[label, value]], shown while it is read.
  property var facts: []
  // [{title, items: [{text, section, id}]}]: what it uses and what uses it,
  // each item by the section and the row it opens.
  property var related: []
  // Commands of its own, read, as the bar takes them, after the usual ones.
  property var extra: []
  property alias details: detailsColumn.data
  default property alias form: formColumn.data
  // The collections it holds, each a titled table read under what it is, in
  // order: [{name, title, note, empty, columns, stacked, tone, rows, add, open,
  // remove, move, makeDefault, commands}], as .temp/settings-contract.md says.
  property var tables: []
  // Child table name -> the id of its row picked; the table picked in last,
  // whose row's commands the bar shows.
  property var childPicks: ({})
  property string childTable: ""
  // The ChildScreen open over the item, or null.
  property Item childScreen: null
  // The id of its row, which it shows: "+" for a new item not saved yet.
  property string key: ""
  // The id of its row as the draft names it, which the table follows once
  // saved; "" keeps `key`.
  property string nextKey: ""

  // Wide enough for what is related to stand beside what it is.
  readonly property bool wide: scroll.width >= 640
  // Something is related to it: else RELATED is left out.
  readonly property bool hasRelated: related.some(group => group.items.length > 0)

  signal back()
  signal removing()
  signal followed(int section, string id)
  // `change` to save at once to the saved config, then `done` (SettingsView.saveNow).
  signal storing(var change, var done)
  // Esc that it had nothing to go back from.
  signal escaped()

  readonly property var commands: childScreen !== null ? [
    { text: I18n.t("settings.back"), role: "back", tip: I18n.t("settings.child_back_tip"), run: () => page.closeScreen() }
  ].concat(childScreen.actions) : editing ? [
    { text: draft && draft.saving ? I18n.t("settings.saving") : I18n.t("config.save"), icon: "enter", role: "primary",
      tip: draft && draft.conflict ? I18n.t("settings.stale") : I18n.t("settings.save_tip"),
      enabled: !!draft && draft.dirty && !draft.conflict && !draft.saving && !Eco.configSaving, run: () => page.save() },
    // A new item in conflict starts over from the saved config.
    { text: I18n.t("config.discard"), icon: "trash", role: "guarded", tip: I18n.t("config.discard_tip"),
      shown: !!draft && draft.dirty && (!draft.isNew || draft.conflict), enabled: !!draft && !draft.saving, run: () => page.discard() },
    { text: I18n.t("dialog.cancel"), icon: "close", tip: I18n.t("settings.cancel_tip"), enabled: !draft || !draft.saving, run: () => page.cancel() }
  ] : [
    { text: I18n.t("settings.back"), role: "back", tip: I18n.t("settings.back_tip"), shown: backable, run: () => page.back() },
    { text: I18n.t("settings.edit"), icon: "edit", role: "primary", tip: I18n.t("settings.edit_tip"), shown: draft !== null, run: () => page.edit() }
  ].concat(childCommands).concat(extra).concat([
    { text: I18n.t("settings.delete"), icon: "trash", role: "guarded", tip: removable ? I18n.t("settings.delete_tip") : keptReason,
      shown: !single && !fixed, enabled: removable, run: () => page.removing() }
  ])

  // Each child table's ADD, the first the main action of an item with no
  // fields; then the commands of the child row picked last.
  readonly property var childCommands: {
    const adds = tables.filter(table => !!table.add).map((table, index) => ({ text: table.add.text, icon: "plus",
      role: draft === null && index === 0 ? "primary" : undefined, tip: table.add.tip || "", enabled: !Eco.configSaving,
      run: () => page.show(table.add.screen, { table: table.name }) }))
    const table = tables.find(entry => entry.name === childTable)
    const row = table ? table.rows.find(entry => entry.id === childPicks[table.name]) || null : null
    return adds.concat(row === null ? [] : rowCommands(table, row))
  }

  // The commands of the child row `row` of `table`: open it, make it the
  // default, move it, the table's own, and remove it; each saves at once.
  function rowCommands(table, row) {
    const at = table.rows.findIndex(entry => entry.id === row.id)
    const isDefault = !!table.makeDefault && table.makeDefault.is(row)
    const idle = !Eco.configSaving
    const commands = []
    if (table.open)
      commands.push({ text: table.open.text, icon: table.open.icon || "enter", tip: table.open.tip || "", run: () => page.openRow(table, row) })
    if (table.makeDefault)
      commands.push({ text: I18n.t("settings.default"), icon: "check", tip: isDefault ? table.makeDefault.isTip : table.makeDefault.tip,
        enabled: idle && !isDefault, run: () => page.store(d => table.makeDefault.change(d, row), table.name, row.id) })
    if (table.move)
      commands.push(
        { text: I18n.t("settings.move_up"), icon: "chevron-up", tip: table.move.up, enabled: idle && at > 0,
          run: () => page.store(d => table.move.change(d, row, -1), table.name, row.id) },
        { text: I18n.t("settings.move_down"), icon: "chevron-down", tip: table.move.down, enabled: idle && at < table.rows.length - 1,
          run: () => page.store(d => table.move.change(d, row, 1), table.name, row.id) })
    return commands.concat(table.commands ? table.commands(row) : []).concat(table.remove ? [
      { text: I18n.t("settings.child_remove"), icon: "trash", role: "guarded",
        tip: row.removable === false ? row.keptReason || "" : I18n.t("settings.child_remove_tip"),
        enabled: idle && row.removable !== false, run: () => page.removeChild(table, row) }
    ] : [])
  }
  function pickChild(table, id) {
    childPicks = Object.assign({}, childPicks, { [table]: id })
    childTable = table
  }
  // The keyboard goes to the child table named `table`, on its row picked.
  function focusChild(table) {
    const block = childTables.itemAt(tables.findIndex(entry => entry.name === table))
    if (block && block.visible)
      block.focusRows()
    else
      page.forceActiveFocus()
  }
  // Saves `change` to the saved config at once; then any child screen closes
  // and the row `pick` of the child table `table` is picked, with the keyboard.
  function store(change, table, pick) {
    storing(change, () => {
      if (page.childScreen !== null)
        page.dropScreen()
      page.pickChild(table, pick)
      Qt.callLater(() => page.focusChild(table))
    })
  }
  // Removes the child row `row` of `table`, asking first when its `remove`
  // has a question; the row after it, or else before it, is picked then.
  function removeChild(table, row) {
    if (!table.remove || row.removable === false || Eco.configSaving)
      return
    const asked = table.remove(row)
    const at = table.rows.findIndex(entry => entry.id === row.id)
    const next = (table.rows[at + 1] || table.rows[at - 1] || { id: "" }).id
    const go = () => page.store(asked.change, table.name, next)
    if (!asked.question)
      return go()
    confirmChild.asked = asked
    confirmChild.then = go
    confirmChild.open()
  }
  // Opens the child screen made by `component`, given `props` and this page.
  function show(component, props) {
    if (childScreen !== null)
      dropScreen()
    childScreen = component.createObject(screenColumn, Object.assign({ page: page }, props))
    Qt.callLater(() => { if (page.childScreen) page.childScreen.takeFocus() })
  }
  function openRow(table, row) {
    if (table.open)
      show(table.open.screen, { table: table.name, row: row })
  }
  // BACK from the child screen: to its child table, unsaved.
  function closeScreen() {
    const table = childScreen.table
    dropScreen()
    Qt.callLater(() => page.focusChild(table))
  }
  function dropScreen() {
    childScreen.destroy()
    childScreen = null
  }

  // Gives the keyboard to the screen: the child screen open, the first field
  // when editing, else the screen itself, whose Esc goes back.
  function takeFocus() {
    if (childScreen !== null) {
      childScreen.takeFocus()
      return
    }
    if (!editing) {
      page.forceActiveFocus()
      return
    }
    const first = formColumn.nextItemInFocusChain(true)
    if (first)
      first.forceActiveFocus()
  }
  function edit() {
    if (draft !== null && childScreen === null)
      editing = true
  }
  function save() {
    if (!draft.save())
      Qt.callLater(focusError)
  }
  // The first stop of the form inside a field with an error takes the keyboard.
  function focusError() {
    const first = formColumn.nextItemInFocusChain(true)
    for (let at = first; at; at = at.nextItemInFocusChain(true)) {
      let field = at
      let inForm = false
      for (; field; field = field.parent) {
        if (field === formColumn) {
          inForm = true
          break
        }
        if (typeof field.error === "string" && field.error !== "") {
          at.forceActiveFocus()
          return
        }
      }
      const next = at.nextItemInFocusChain(true)
      if (!inForm || next === first)
        return
    }
  }
  // Runs `then`, which drops the changes, once the user agrees when there are any.
  function confirmDrop(then) {
    if (draft === null || !draft.touched)
      return then()
    confirmDropping.then = then
    confirmDropping.open()
  }
  // CANCEL: out of editing, asking first when that drops changes.
  function cancel() { confirmDrop(drop) }
  // DISCARD: the saved config read again, asking first.
  function discard() { confirmDrop(() => draft.reset()) }
  // Out of editing, dropping the changes; a new item, never saved, goes back.
  function drop() {
    const fresh = draft !== null && draft.isNew
    if (draft)
      draft.end()
    editing = false
    if (fresh)
      back()
  }
  // Esc: out of the child screen, out of editing, then back; a save under way keeps it. Returns whether
  // it took the key.
  function leave() {
    if (childScreen !== null)
      closeScreen()
    else if (editing && draft !== null && draft.saving)
      return true
    else if (editing)
      cancel()
    else if (backable)
      back()
    else
      return false
    return true
  }

  // Editing starts from the saved config, unless a new item began it already.
  onEditingChanged: {
    if (editing && draft !== null && !draft.active)
      draft.begin(null)
    Qt.callLater(takeFocus)
  }
  Connections {
    target: page.draft
    function onSaved() { page.editing = false }
  }
  Shortcut { enabled: page.visible && page.draft !== null && !page.editing; sequence: "Ctrl+E"; onActivated: page.edit() }
  ConfirmDialog {
    id: confirmDropping
    property var then: null
    title: I18n.t("settings.close_title")
    question: I18n.t("settings.drop_question", { name: page.title || page.placeholder })
    confirmText: I18n.t("settings.discard_changes")
    cancelText: I18n.t("settings.keep_editing")
    onConfirmed: then()
  }
  Shortcut { enabled: page.visible && page.editing; sequence: "Ctrl+S"; onActivated: page.save() }
  ConfirmDialog {
    id: confirmChild
    property var asked: ({ title: "", question: "", warning: "" })
    property var then: null
    title: asked.title
    question: asked.question
    warning: asked.warning || ""
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: then()
  }

  // A titled frame of the read screen.
  component Frame: Rectangle {
    id: frame
    property string title
    default property alias fields: inner.data
    implicitHeight: inner.implicitHeight + 46 + heading.implicitHeight
    color: "transparent"
    border.color: Theme.line
    Caption { id: heading; x: 16; y: 16; text: frame.title.toUpperCase() }
    ColumnLayout {
      id: inner
      anchors { left: parent.left; right: parent.right; top: heading.bottom; leftMargin: 16; rightMargin: 16; topMargin: 12 }
      spacing: 10
    }
  }

  ScrollPage {
    id: scroll
    anchors.fill: parent
    contentHeight: content.implicitHeight

    ColumnLayout {
      id: content
      width: Math.min(scroll.width, 880)
      spacing: 14

      // What it is, and how far its edit is: the item of a single section, which
      // its title names, has the line only while it is edited.
      Row {
        visible: page.kind !== "" || page.childScreen !== null || page.editing
        spacing: 12
        // Over a child screen, the item it is in.
        Label {
          visible: text !== ""
          text: (page.childScreen !== null ? page.title || page.placeholder : page.kind).toUpperCase()
          color: Theme.dim
          font.pixelSize: 10
          font.letterSpacing: 3
        }
        Label {
          visible: page.childScreen === null && !!page.draft && page.draft.dirty && !page.draft.saving
          text: !page.draft ? "" : page.draft.conflict ? I18n.t("settings.mark_changed")
            : page.draft.isNew ? I18n.t("settings.mark_new") : I18n.t("settings.mark_unsaved")
          color: page.draft && page.draft.conflict ? Theme.warning : Theme.primary
          font.pixelSize: 10
          font.letterSpacing: 2
        }
        Label { visible: page.editing; text: I18n.t("settings.mark_editing"); color: Theme.secondary; font.pixelSize: 10; font.letterSpacing: 3 }
      }
      DecodeLabel {
        Layout.fillWidth: true
        value: page.childScreen !== null ? page.childScreen.title : page.title || page.placeholder
        color: page.title || page.childScreen !== null ? Theme.foreground : Theme.dim
        font.pixelSize: 22
        font.letterSpacing: 2
        elide: Text.ElideRight
      }
      FieldError {
        Layout.fillWidth: true
        text: !page.draft ? "" : page.draft.conflict ? I18n.t("settings.stale") : page.draft.issue
      }

      // Read: what it is, then what is related to it, side by side when wide.
      GridLayout {
        Layout.fillWidth: true
        visible: !page.editing && page.childScreen === null && (page.facts.length > 0 || page.hasRelated)
        columns: page.wide && page.hasRelated ? 2 : 1
        columnSpacing: 20
        rowSpacing: 20

        Frame {
          title: I18n.t("settings.about")
          Layout.fillWidth: true
          Layout.alignment: Qt.AlignTop
          visible: page.facts.length > 0
          GridLayout {
            Layout.fillWidth: true
            columns: 2
            columnSpacing: 14
            rowSpacing: 10
            Repeater {
              model: page.editing ? [] : page.facts.reduce((cells, fact) => cells.concat([{ label: true, text: fact[0] }, { label: false, text: fact[1] }]), [])
              delegate: DecodeLabel {
                required property var modelData
                Layout.fillWidth: !modelData.label
                Layout.preferredWidth: modelData.label ? -1 : 1
                Layout.alignment: Qt.AlignTop
                value: modelData.label ? modelData.text.toUpperCase() : (modelData.text === "" ? "—" : modelData.text)
                color: modelData.label ? Theme.dim : Theme.foreground
                font.pixelSize: modelData.label ? 10 : 12
                font.letterSpacing: modelData.label ? 2 : 0
                wrapMode: modelData.label ? Text.NoWrap : Text.Wrap
              }
            }
          }
        }

        Frame {
          title: I18n.t("settings.related")
          visible: page.hasRelated
          Layout.preferredWidth: page.wide ? 300 : content.width
          Layout.maximumWidth: Layout.preferredWidth
          Layout.alignment: Qt.AlignTop
          Repeater {
            model: page.related.filter(group => group.items.length > 0)
            delegate: ColumnLayout {
              id: group
              required property var modelData
              Layout.fillWidth: true
              spacing: 6
              Caption { Layout.fillWidth: true; wrapMode: Text.Wrap; text: group.modelData.title.toUpperCase() }
              Flow {
                Layout.fillWidth: true
                spacing: 6
                Repeater {
                  model: group.modelData.items
                  delegate: Chip {
                    required property var modelData
                    text: modelData.text
                    icon: "chevron-right"
                    onClicked: page.followed(modelData.section, modelData.id)
                  }
                }
              }
            }
          }
        }
      }

      // Read: its long texts, across the screen, so a text keeps its own lines.
      ColumnLayout {
        id: detailsColumn
        Layout.fillWidth: true
        visible: !page.editing && page.childScreen === null && children.length > 0
        spacing: 12
      }

      // Read: the collections it holds, each a table whose rows are picked.
      ColumnLayout {
        Layout.fillWidth: true
        visible: !page.editing && page.childScreen === null && page.tables.length > 0
        spacing: 20
        Repeater {
          id: childTables
          // By count: tables rebuilt as the config changes keep their rows and keyboard.
          model: page.tables.length
          delegate: ColumnLayout {
            id: block
            required property int index
            readonly property var table: page.tables[index]
            function focusRows() { rows.focusRows() }
            Layout.fillWidth: true
            spacing: 8
            Caption { text: block.table.title.toUpperCase() }
            ItemNote { visible: text !== ""; text: block.table.note || "" }
            DataTable {
              id: rows
              Layout.fillWidth: true
              Layout.preferredHeight: implicitHeight
              picks: true
              columns: block.table.columns
              stacked: block.table.stacked || [[0]]
              tone: block.table.tone || ((entry, column) => "")
              rows: block.table.rows
              cells: entry => entry.cells
              selectedId: page.childPicks[block.table.name] || ""
              emptyText: block.table.empty || ""
              onPicked: entry => page.pickChild(block.table.name, entry.id)
              onOpened: entry => page.openRow(block.table, entry)
              onRemoveRequested: entry => page.removeChild(block.table, entry)
              onBackRequested: if (!page.leave()) page.escaped()
            }
          }
        }
      }

      // Over the item: the child screen open.
      ColumnLayout {
        id: screenColumn
        Layout.fillWidth: true
        visible: page.childScreen !== null
        spacing: 14
      }

      // Edited: its own fields.
      ColumnLayout {
        id: formColumn
        Layout.fillWidth: true
        visible: page.editing
        spacing: 14
      }
    }
  }
}
