pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SettingsView is the settings one section at a time, each a table of its
// items: a click picks one, a double click or Enter opens it on a screen of
// its own, read, where it may be edited and saved alone. A section that is one
// item opens it at once. What is picked or open decides the commands the bar
// shows. Removing an item, or moving one, saves at once, after asking.
Item {
  id: view

  // The section on screen, 0 to 7.
  property int page: 0
  // A section asked for, as when a related item is followed.
  signal wanted(int page)
  // Esc with nothing left to go back from.
  signal closeRequested()

  // The sections' places, for links: settings.link(settings.at.answers, "assistant").
  readonly property var at: ({ audio: 0, models: 1, transcription: 2, answers: 3, skills: 4, translation: 5, sessions: 6, interface: 7 })
  readonly property var sections: [audio, models, transcription, answers, skills, translation, sessions, interfaceSection]
  AudioSection { id: audio; settings: view }
  ModelsSection { id: models; settings: view }
  TranscriptionSection { id: transcription; settings: view }
  AnswersSection { id: answers; settings: view }
  SkillsSection { id: skills; settings: view }
  TranslationSection { id: translation; settings: view }
  SessionsSection { id: sessions; settings: view }
  InterfaceSection { id: interfaceSection; settings: view }

  // A chat model is registered: until then answers, skills and translation have nothing to run on.
  readonly property bool chats: Eco.config !== null && Draft.chats(Eco.config)

  // Section index -> the id of the row picked, and of the item open ("+" for a
  // new one, which opens to be written).
  property var picked: ({})
  property var shown: ({})
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

  // The commands of the table of section `index`: add, and, with a row
  // picked, view and delete it, among the section's own.
  function tableCommands(index) {
    const section = sections[index]
    if (section.chats && !chats)
      return [{ text: I18n.t("settings.no_chat_register"), icon: "plus", role: "primary", run: () => view.create(at.models) }]
    const config = Eco.config
    const row = config === null ? null : section.rows(config).find(entry => entry.id === (picked[index] || "")) || null
    return [
      { text: section.add, icon: "plus", role: "primary", shown: section.add !== "" && config !== null, run: () => view.create(index) },
      { text: I18n.t("settings.view"), icon: "enter", tip: I18n.t("settings.view_tip"), shown: row !== null, run: () => view.open(index, row.id) }
    ].concat(config === null ? [] : section.commands(config, row)).concat([
      { text: I18n.t("settings.delete"), icon: "trash", role: "guarded", tip: row && row.removable === false ? row.keptReason || "" : I18n.t("settings.delete_tip"),
        shown: row !== null && section.remove !== null && !row.fixed, enabled: row !== null && row.removable !== false, run: () => view.remove(index, row) }
    ])
  }

  // Opens the item `id` of the section `index`; "+", a new one, to be written.
  function open(index, id) {
    picked = setOf(picked, index, id)
    shown = setOf(shown, index, id)
  }
  // Shows the section `index` to open its item `id` there, unless another item
  // open there has changes not saved: then that one stays, and the window's
  // message says why. Returns whether `id` may open.
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
    if (reach(index, id) && !sections[index].single)
      open(index, id)
  }
  // Choosing a section elsewhere starts the way back over.
  function forget() { history = [] }

  // Asks before deleting `row` of section `index`, saying what follows, then saves at once.
  function remove(index, row) {
    if (row.fixed || row.removable === false || sections[index].remove === null)
      return
    const asked = sections[index].remove(Eco.config, row)
    confirmRemove.asked = asked
    confirmRemove.after = () => {
      if (view.shown[index] === row.id)
        view.shown = view.setOf(view.shown, index, "")
      view.picked = view.setOf(view.picked, index, "")
      Qt.callLater(view.takeFocus)
    }
    confirmRemove.open()
  }
  // Saves the saved config with `change` applied at once, then calls `done`;
  // what keeps it from saving, or the daemon's refusal, shows as the window's
  // message, on the masthead line.
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
    return openPage !== null && openPage.leave()
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
  // item that does not save stops it: it shows, and the window's message says why.
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
        readonly property bool waiting: section.chats && !view.chats
        readonly property var rows: Eco.config === null || waiting ? [] : section.rows(Eco.config)
        // The id of the item open: the one item of a single section.
        readonly property string open: waiting ? "" : section.single ? (rows.length ? rows[0].id : "") : view.shown[index] || ""
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
        function focusRows() { table.focusRows() }
        anchors.fill: parent
        shown: view.page === index

        // The table of the section's items, or what it waits for.
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
          Kicker {
            Layout.fillWidth: true
            visible: stage.waiting
            text: I18n.t("settings.no_chat")
          }
          Label {
            Layout.fillWidth: true
            visible: text !== ""
            text: stage.waiting ? I18n.t("settings.no_chat_note") : stage.section.note
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
          DataTable {
            id: table
            Layout.fillWidth: true
            Layout.fillHeight: true
            visible: !stage.waiting
            picks: true
            active: stage.shown && stage.open === ""
            columns: stage.section.columns
            stacked: stage.section.stacked
            rows: stage.rows
            cells: row => row.cells || []
            idOf: row => row.id
            selectedId: view.picked[stage.index] || ""
            loading: Eco.config === null && Eco.connected
            emptyText: Eco.config === null ? (Eco.connected ? I18n.t("config.loading") : I18n.t("start.offline")) : I18n.t("settings.empty")
            onPicked: row => view.picked = view.setOf(view.picked, stage.index, row.id)
            onOpened: row => view.open(stage.index, row.id)
            onRemoveRequested: row => view.remove(stage.index, row)
            onBackRequested: view.closeRequested()
          }
          Item { Layout.fillHeight: true; visible: stage.waiting }
        }

        // The item open, on the screen of its kind.
        Loader {
          id: screen
          anchors.fill: parent
          active: stage.open !== ""
          readonly property var row: stage.open === "+" ? { id: "+", file: "" } : stage.rows.find(entry => entry.id === stage.open) || null
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
            item.single = stage.section.single
            item.backable = Qt.binding(() => !stage.section.single || view.history.length > 0)
            item.fixed = Qt.binding(() => screen.row !== null && !!screen.row.fixed)
            item.removable = Qt.binding(() => screen.row !== null && screen.row.removable !== false)
            item.keptReason = Qt.binding(() => screen.row !== null ? screen.row.keptReason || "" : "")
            item.editing = item.key === "+"
            // The keyboard goes with the screen that opens.
            Qt.callLater(() => { if (screen.item) screen.item.takeFocus() })
            item.back.connect(() => view.close(stage.index))
            item.followed.connect((section, id) => view.follow(section, id))
            item.removing.connect(() => view.remove(stage.index, screen.row))
            item.storing.connect((change, done) => view.saveNow(change, done))
            item.escaped.connect(() => view.closeRequested())
            // Saved, under a new name or for the first time, the screen follows it.
            if (item.draft)
              item.draft.saved.connect(() => {
                const key = screen.item.nextKey
                if (key === "" || stage.section.single)
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
    property var asked: ({ title: "", question: "", warning: "", change: null })
    property var after: null
    title: asked.title
    question: asked.question
    warning: asked.warning || ""
    confirmText: I18n.t("settings.remove_confirm")
    onConfirmed: view.saveNow(asked.change, after)
  }
}
