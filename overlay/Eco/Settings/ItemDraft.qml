import QtQuick
import Eco.Core

// ItemDraft is the edit of one item of the settings: a copy of the whole saved
// config (Eco.config) that its form changes only where the item is, so a save
// sends what is saved plus this item's change, renames and removals following
// it wherever they reach. While unchanged it follows the saved config; when that
// changes under a changed draft, its changes are made again on the new one
// while the lists the item lives in, and those its changes reached, are as they
// were there, and otherwise it saves no more until it is read again.
QtObject {
  id: item

  // The config paths the item owns ("models.2", "stt"): a problem at one of
  // them or under it shows on its field, by its full path, in `errors`; any
  // other is the page's `issue`.
  property var paths: []
  // The whole config being edited, or null while the item is only read.
  property var draft: null
  // The saved config the draft was copied from.
  property var base: null
  // An item not saved yet, made by the change `begin` was given.
  property var make: null
  // The changes made to it, in order, to make again on a newer saved config.
  property var changes: []
  readonly property bool isNew: make !== null
  // What is wrong in what is being typed that the draft cannot hold yet, by
  // path, as text: a model's extra fields that are not JSON.
  property var typed: ({})
  // A save was tried since it began: missing values show as problems too.
  property bool tried: false
  property bool saving: false
  // What the daemon refused the last save for, or a problem off the item's fields.
  property string issue: ""

  readonly property bool active: draft !== null
  readonly property bool dirty: active && (isNew || Object.keys(typed).length > 0 || JSON.stringify(draft) !== JSON.stringify(base))
  // Something was changed or typed since it began, which dropping it loses.
  readonly property bool touched: dirty && (!isNew || changes.length > 0 || Object.keys(typed).length > 0)
  readonly property bool conflict: active && dirty && JSON.stringify(Eco.config) !== JSON.stringify(base)
  // What keeps the draft from saving, on the item's fields and elsewhere.
  readonly property var problems: active ? Draft.problems(draft) : []
  readonly property var own: problems.filter(problem => owns(problem.path))
  // Messages by full path: what was typed wrong, and once a save was tried, what is missing.
  readonly property var errors: {
    const map = Object.assign({}, typed)
    own.filter(problem => tried || problem.typed).forEach(problem => {
      if (map[problem.path] === undefined)
        map[problem.path] = I18n.t(problem.key)
    })
    return map
  }

  // Saved: the page reads it again.
  signal saved()
  // Begun from the saved config, or read again: what is typed shows the draft.
  signal began()

  function owns(path) {
    return paths.some(prefix => path === prefix || path.startsWith(prefix + "."))
  }

  // Starts editing from the saved config; `change`, for an item not saved yet,
  // adds it to the copy.
  function begin(change) {
    make = change || null
    const next = JSON.parse(JSON.stringify(Eco.config))
    if (make)
      make(next)
    draft = next
    base = JSON.parse(JSON.stringify(Eco.config))
    changes = []
    typed = ({})
    tried = false
    issue = ""
    began()
  }
  // Reads the saved config again, dropping the changes; a new item starts over.
  function reset() { begin(make) }
  function end() {
    saving = false
    draft = null
    base = null
    make = null
    changes = []
    typed = ({})
    tried = false
    issue = ""
  }
  // Makes its changes again on the saved config, which changed under them, when
  // the lists it owns or reached are untouched there; otherwise it stays in conflict.
  function rebase() {
    const next = Draft.rebased(Eco.config, base, draft, paths, make, changes)
    if (next === null)
      return
    draft = next
    base = JSON.parse(JSON.stringify(Eco.config))
  }

  // Applies `change` to a copy of the draft.
  function update(change) {
    const next = JSON.parse(JSON.stringify(draft))
    change(next)
    draft = next
    changes = changes.concat([change])
    issue = ""
  }
  // Marks what is typed at `path` as wrong with `message`, or right with "".
  function type(path, message) {
    const next = Object.assign({}, typed)
    if (message)
      next[path] = message
    else
      delete next[path]
    typed = next
  }

  // Saves the draft, unless something keeps it from saving: then the item's
  // problems show on its fields and any other as the issue; another save under
  // way, from any item, keeps it waiting. Returns whether a save started.
  function save() {
    if (!dirty || saving || conflict || Eco.configSaving)
      return false
    tried = true
    if (own.length > 0 || Object.keys(typed).length > 0)
      return false
    const other = problems.find(problem => !owns(problem.path))
    if (other) {
      issue = I18n.t(other.key)
      return false
    }
    issue = ""
    saving = true
    draft = Draft.normalised(draft)
    Eco.saveConfig(draft)
    return true
  }

  property Connections daemon: Connections {
    target: Eco
    function onConfigChanged() {
      // Read here, not from `conflict`, whose binding may not have seen the change yet.
      if (!item.active || item.saving || JSON.stringify(Eco.config) === JSON.stringify(item.base))
        return
      if (!item.dirty)
        item.begin(item.make)
      else
        item.rebase()
    }
    function onConfigSaveSucceeded() {
      if (!item.saving)
        return
      item.saving = false
      // What was saved is what the daemon holds now; it sends it whole again.
      if (item.draft !== null)
        Eco.config = item.draft
      Eco.requestConfig()
      item.saved()
      item.end()
    }
    function onConfigSaveFailed(detail) {
      if (!item.saving)
        return
      item.saving = false
      item.issue = detail
    }
  }
}
