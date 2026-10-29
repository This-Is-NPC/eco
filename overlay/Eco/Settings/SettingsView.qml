pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SettingsView is the settings one section at a time, each a table of its
// items, or a body of its own in the table's place: a click picks one, a
// double click, Enter or Space opens it on a screen of its own, read, where it
// may be edited and saved alone; EDIT or
// Ctrl+E opens the one picked to be written, and back to the table once saved
// or cancelled. What is picked or open decides the commands the bar shows.
// Removing an item saves at once, after asking.
Item {
  id: view

  // The section on screen, 0 to 8.
  property int page: 0
  // A section asked for, as when a related item is followed.
  signal wanted(int page)
  // Esc with nothing left to go back from.
  signal closeRequested()

  // The sections' places, for links: settings.link(settings.at.prompts, "rules").
  readonly property var at: ({ audio: 0, models: 1, languages: 2, kinds: 3, prompts: 4, context: 5, skills: 6, general: 7, appearance: 8 })
  readonly property var sections: [audio, models, languages, kinds, prompts, context, skills, general, appearance]
  AudioSection { id: audio; settings: view }
  ModelsSection { id: models; settings: view }
  LanguagesSection { id: languages; settings: view }
  KindsSection { id: kinds; settings: view }
  PromptsSection { id: prompts; settings: view }
  ContextSection { id: context; settings: view }
  SkillsSection { id: skills; settings: view }
  GeneralSection { id: general; settings: view }
  AppearanceSection { id: appearance; settings: view }

  // A chat model is registered: until then answers and skills have nothing to
  // run on, and the sections that need one say so above their tables.
  readonly property bool chats: Eco.config !== null && Draft.chats(Eco.config)

  // Section index -> the id of the row picked, of the item open ("+" for a
  // new one, which opens to be written), and of the item EDIT opened to be
  // written, which goes back to the table once saved or cancelled.
  property var picked: ({})
  property var shown: ({})
  property var writing: ({})
  // The screens related items were followed from, the last last.
  property var history: []
  // The sections whose open item has changes not saved, and those whose open
  // item's changes were overtaken by a save elsewhere.
  property var unsaved: []
  property var stale: []
  readonly property bool dirty: unsaved.length > 0
  readonly property bool conflict: stale.length > 0
  // What runs once SAVE AND CLOSE has saved every item, while it saves them.
  property var finish: null

  // The page of the item open on the section on screen, or null.
  property Item openPage: null
  readonly property var commands: openPage ? openPage.commands : tableCommands(page)

  function setOf(map, index, value) { return Object.assign({}, map, { [index]: value }) }
  // The section indices `list` with `index` in it or not.
  function mark(list, index, on) { return list.filter(at => at !== index).concat(on ? [index] : []) }
  // A switch as read: on or off.
  function onOff(on) { return I18n.t(on ? "settings.fact_on" : "settings.fact_off") }

  // The commands of the table of section `index`: add; view, edit and delete
  // the row picked, among the section's own, each there and off, saying why,
  // until a row is picked.
  function tableCommands(index) {
    const section = sections[index]
    const config = Eco.config
    const rows = config === null ? [] : section.rows(config)
    const row = rows.find(entry => entry.id === (picked[index] || "")) || null
    // Its rows open screens: VIEW and EDIT are there.
    const opens = section.file !== "" || rows.some(entry => !!entry.file)
    const pick = I18n.t("settings.pick_row_tip")
    return [
      { text: section.add, short: section.addShort, icon: "plus", role: "primary", shown: section.add !== "", enabled: config !== null, run: () => view.create(index) },
      { text: I18n.t("settings.view"), icon: "chevron-right", tip: row === null ? pick : I18n.t("settings.view_tip"), shown: opens,
        enabled: row !== null && view.screenOf(index, row) !== "", run: () => view.enter(index, row) },
      { text: I18n.t("settings.edit"), icon: "edit", shown: opens,
        tip: row === null ? pick : row.edits === false ? row.noEdit || "" : I18n.t("settings.edit_row_tip"),
        enabled: view.edits(index, row), run: () => view.edit(index, row) }
    ].concat(config === null ? [] : section.commands(config, row)).concat([
      { text: I18n.t("settings.delete"), icon: "trash", role: "guarded", shown: section.remove !== null,
        tip: row === null ? pick : row.fixed ? I18n.t("settings.kept_fixed_tip") : row.removable === false ? row.keptReason || "" : I18n.t("settings.delete_tip"),
        enabled: row !== null && !row.fixed && row.removable !== false, run: () => view.remove(index, row) }
    ])
  }

  // The screen file `row` of section `index` opens, or "" when it has none.
  function screenOf(index, row) { return row.file || sections[index].file }
  // The row picked in section `index`, or null.
  function pickedRow(index) {
    const section = sections[index]
    if (Eco.config === null)
      return null
    return section.rows(Eco.config).find(entry => entry.id === (picked[index] || "")) || null
  }
  // Whether EDIT may open `row` of section `index` to be written.
  function edits(index, row) {
    return row !== null && screenOf(index, row) !== "" && row.edits !== false && !Eco.configSaving
  }
  // A row's VIEW, Enter, Space or double click: it opens on its screen, when it has one.
  function enter(index, row) {
    if (screenOf(index, row) !== "")
      open(index, row.id)
  }
  // EDIT: the row opens to be written, and back to the table once done.
  function edit(index, row) {
    if (edits(index, row))
      open(index, row.id, true)
  }
  // Opens the item `id` of the section `index`, to be written when `write`;
  // "+", a new one, is always written.
  function open(index, id, write) {
    picked = setOf(picked, index, id)
    writing = setOf(writing, index, write === true ? id : "")
    shown = setOf(shown, index, id)
  }
  // Shows the section `index` to open its item `id` there, unless another item
  // open there has changes not saved: then that one stays, and an error toast
  // says why. Returns whether `id` may open.
  function reach(index, id) {
    wanted(index)
    const stage = stages.itemAt(index)
    if (!stage || !stage.dirty || stage.open === id)
      return true
    Eco.say(I18n.t("settings.finish_first", { name: stage.name }), true)
    return false
  }
  // Closes the item open in section `index`: back to the screen it was
  // followed from, or to the table.
  function close(index) {
    shown = setOf(shown, index, "")
    writing = setOf(writing, index, "")
    if (history.length > 0) {
      const from = history[history.length - 1]
      history = history.slice(0, -1)
      wanted(from.section)
      if (from.id !== "")
        open(from.section, from.id)
      Qt.callLater(takeFocus)
      return
    }
    Qt.callLater(takeFocus)
  }
  // Adds a new item to section `index`, not saved yet, and opens it to be written.
  function create(index) {
    if (reach(index, "+"))
      open(index, "+")
  }
  // A link to the item `id` of the section `index`, as related items are.
  function link(index, id, text) { return { section: index, id: id, text: text } }
  // Opens a related item in its own section; BACK there comes back here.
  function follow(index, id) {
    history = history.concat([{ section: page, id: shown[page] || "" }])
    if (reach(index, id))
      open(index, id)
  }
  // Choosing a section elsewhere starts the way back over.
  function forget() { history = [] }

  // Asks before deleting `row` of section `index`, saying what follows; then
  // closes its screen, so its tables are gone before the save changes their
  // rows, and saves at once.
  function remove(index, row) {
    if (row.fixed || row.removable === false || sections[index].remove === null)
      return
    const asked = sections[index].remove(Eco.config, row)
    confirmRemove.asked = asked
    confirmRemove.then = () => {
      if (view.shown[index] === row.id)
        view.shown = view.setOf(view.shown, index, "")
      view.saveNow(asked.change, () => {
        view.picked = view.setOf(view.picked, index, "")
        Qt.callLater(view.takeFocus)
      })
    }
    confirmRemove.open()
  }
  // Saves the saved config with `change` applied at once, then calls `done`;
  // what keeps it from saving, or the daemon's refusal, shows in an error toast.
  function saveNow(change, done) {
    if (quick.saving || Eco.configSaving)
      return
    quick.done = done || null
    quick.begin(change)
    // Refused before it was sent, it has said why.
    if (!quick.save() && quick.active)
      quick.end()
  }

  // Gives the keyboard to the section on screen: its open item, or its rows.
  function takeFocus() {
    if (openPage)
      openPage.takeFocus()
    else if (stages.itemAt(page))
      stages.itemAt(page).focusRows()
  }
  // Esc: out of the open item; returns whether there was one to leave.
  function leave() {
    if (openPage === null)
      return false
    openPage.leave()
    return true
  }
  // The open items with changes not saved, the one on screen first.
  function changed() {
    const found = []
    for (let i = 0; i < stages.count; i++) {
      const stage = stages.itemAt((page + i) % stages.count)
      if (stage && stage.dirty)
        found.push(stage)
    }
    return found
  }
  // SAVE AND CLOSE: saves each item with changes in turn, then runs `done`. An
  // item that does not save stops it: it shows, and an error toast says why.
  function saveAll(done) {
    finish = done
    saveNext()
  }
  function saveNext() {
    const first = changed()[0]
    if (!first) {
      const done = finish
      finish = null
      if (done)
        done()
      return
    }
    wanted(first.index)
    const page = first.screenItem
    if (page.draft.save())
      return
    finish = null
    Eco.say(page.draft.conflict ? I18n.t("settings.stale") : page.draft.issue || I18n.t("settings.fix_first", { name: first.name }), true)
    Qt.callLater(page.focusError)
  }
  // Drops every change not saved: each item being written stops.
  function discardChanged() {
    changed().forEach(stage => stage.screenItem.drop())
  }

  ItemDraft { id: quick; property var done: null }
  // Ctrl+E on a table edits the row picked; on an item open, ItemPage's own takes it.
  Shortcut {
    sequence: "Ctrl+E"
    enabled: view.visible && view.openPage === null && view.edits(view.page, view.pickedRow(view.page))
    onActivated: view.edit(view.page, view.pickedRow(view.page))
  }
  // SAVE AND CLOSE goes on once a save is through, and stops at one refused.
  Connections {
    target: Eco
    enabled: view.finish !== null
    function onConfigSaveSucceeded() { Qt.callLater(view.saveNext) }
    function onConfigSaveFailed(detail) {
      view.finish = null
      Eco.say(detail, true)
    }
  }
  Connections {
    target: quick
    function onSaved() {
      if (quick.done)
        quick.done()
      quick.done = null
    }
    function onIssueChanged() {
      if (quick.issue === "" || quick.saving)
        return
      Eco.say(quick.issue, true)
      quick.end()
    }
  }

  // Esc that nothing inside took (an open list or dialog takes its own) goes back.
  Item {
    anchors.fill: parent
    focus: true
    Keys.onEscapePressed: event => {
      if (!view.leave())
        view.closeRequested()
    }

    Repeater {
      id: stages
      model: view.sections.length
      delegate: Stage {
        id: stage
        required property int index
        readonly property var section: view.sections[index]
        // It needs a chat model and none is registered: it says so, and how to fix it.
        readonly property bool lacking: section.chats && !view.chats
        readonly property var rows: Eco.config === null ? [] : section.rows(Eco.config)
        // The id of the item open.
        readonly property string open: view.shown[index] || ""
        readonly property Item screenItem: screen.item
        readonly property bool dirty: screen.item !== null && screen.item.draft !== null && screen.item.draft.dirty
        readonly property bool conflict: dirty && screen.item.draft.conflict
        // The open item, as messages name it.
        readonly property string name: screen.item ? screen.item.title || screen.item.placeholder : ""
        onDirtyChanged: view.unsaved = view.mark(view.unsaved, index, dirty)
        onConflictChanged: view.stale = view.mark(view.stale, index, conflict)
        Component.onDestruction: {
          view.unsaved = view.mark(view.unsaved, index, false)
          view.stale = view.mark(view.stale, index, false)
        }
        // The section's own body, when it has one, takes the keyboard instead of the table.
        function focusRows() {
          if (body.item === null)
            table.focusRows()
          else if (typeof body.item.takeFocus === "function")
            body.item.takeFocus()
          else
            body.item.forceActiveFocus()
        }
        anchors.fill: parent
        shown: view.page === index
        onShownChanged: if (shown && index === view.at.general) Eco.checkStaleUpdate()
        Component.onCompleted: if (shown && index === view.at.general) Eco.checkStaleUpdate()

        // The table of the section's items, under what it lacks.
        ColumnLayout {
          anchors.fill: parent
          visible: stage.open === ""
          spacing: 12
          DecodeLabel {
            value: stage.section.name.toUpperCase()
            color: Theme.primary
            font.pixelSize: 16
            font.letterSpacing: 4
          }
          ColumnLayout {
            Layout.fillWidth: true
            visible: stage.lacking
            spacing: 8
            Kicker {
              Layout.fillWidth: true
              text: I18n.t("settings.no_chat")
            }
            Label {
              Layout.fillWidth: true
              text: I18n.t("settings.no_chat_note")
              color: Theme.dim
              font.pixelSize: 11
              wrapMode: Text.Wrap
            }
            Chip {
              icon: "plus"
              text: I18n.t("settings.no_chat_register")
              onClicked: view.create(view.at.models)
            }
          }
          Label {
            Layout.fillWidth: true
            visible: text !== ""
            text: stage.section.note
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
          DataTable {
            id: table
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: stage.section.body === null
            picks: true
            active: stage.shown && stage.open === ""
            columns: stage.section.columns
            stacked: stage.section.stacked
            rows: stage.rows
            cells: row => row.cells || []
            idOf: row => row.id
            section: stage.section.group
            dimmed: row => row.dim === true
            tools: stage.section.tools
            toolsWidth: stage.section.toolsWidth
            toolsTitle: stage.section.toolsTitle
            toolsAlways: stage.section.tools !== null
            toolsLive: stage.section.tools !== null
            selectedId: view.picked[stage.index] || ""
            loading: Eco.config === null && Eco.connected
            emptyText: Eco.config === null ? (Eco.connected ? I18n.t("config.loading") : I18n.t("start.offline")) : I18n.t("settings.empty")
            onPicked: row => view.picked = view.setOf(view.picked, stage.index, row.id)
            onOpened: row => view.enter(stage.index, row)
            onRemoveRequested: row => view.remove(stage.index, row)
            onBackRequested: view.closeRequested()
          }
          // The section's own body in place of the table, made once with the
          // view and its section.
          Item {
            id: body
            property Item item: null
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: item !== null
            // It fills the room the table would.
            Component.onCompleted: {
              if (stage.section.body === null)
                return
              item = stage.section.body.createObject(body, { settings: view, section: stage.section })
              item.width = Qt.binding(() => body.width)
              item.height = Qt.binding(() => body.height)
            }
          }
        }

        // The item open, on the screen of its kind.
        Loader {
          id: screen
          anchors.fill: parent
          active: stage.open !== ""
          // A new item, never deleted: on the screen ADD opens.
          readonly property var row: stage.open === "+" ? { id: "+", file: stage.section.adds, fixed: true } : stage.rows.find(entry => entry.id === stage.open) || null
          onRowChanged: load()
          onActiveChanged: load()
          function load() {
            if (!active) {
              source = ""
              return
            }
            // Gone from the config, as when removed elsewhere: back, once a
            // rename saved meanwhile has had its turn.
            if (row === null) {
              Qt.callLater(() => { if (screen.active && screen.row === null && Eco.config !== null) view.close(stage.index) })
              return
            }
            if (item && item.key === row.id)
              return
            setSource(row.file || stage.section.file, { settings: view, section: stage.section, key: row.id })
          }
          onLoaded: {
            item.fixed = Qt.binding(() => screen.row !== null && !!screen.row.fixed)
            item.removable = Qt.binding(() => screen.row !== null && screen.row.removable !== false)
            item.keptReason = Qt.binding(() => screen.row !== null ? screen.row.keptReason || "" : "")
            // A new item is written, unless it has nothing to write: it is
            // chosen; so is one EDIT opened, which then goes back to the table.
            item.returns = item.key !== "+" && view.writing[stage.index] === item.key
            item.editing = item.draft !== null && (item.key === "+" || view.writing[stage.index] === item.key)
            // The keyboard goes with the screen that opens.
            Qt.callLater(() => { if (screen.item) screen.item.takeFocus() })
            item.back.connect(() => view.close(stage.index))
            item.followed.connect((section, id) => view.follow(section, id))
            item.removing.connect(() => view.remove(stage.index, screen.row))
            item.storing.connect((change, done) => view.saveNow(change, done))
            // Saved, under a new name or for the first time, the screen follows it.
            if (item.draft)
              item.draft.saved.connect(() => {
                const key = screen.item.nextKey
                if (key === "")
                  return
                screen.item.key = key
                view.shown = view.setOf(view.shown, stage.index, key)
                view.picked = view.setOf(view.picked, stage.index, key)
              })
          }
        }
        Binding {
          when: stage.shown
          target: view
          property: "openPage"
          value: screen.item
          restoreMode: Binding.RestoreNone
        }
      }
    }
  }

  ConfirmDialog {
    id: confirmRemove
    property var asked: ({ title: "", question: "", warning: "" })
    property var then: null
    title: asked.title
    question: asked.question
    warning: asked.warning || ""
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: then()
  }
}
