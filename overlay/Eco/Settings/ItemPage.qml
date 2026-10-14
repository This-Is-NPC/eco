pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ItemPage is the screen of one item of the settings: what kind it is, its
// name and how far its edit is; read, what it is (`facts`, then any long
// `details`) beside what is `related` to it, each related item a link that
// opens where it is set; edited, its form. Its commands, for the bar, go back,
// edit and delete it, or, editing, save, discard and cancel, then its own
// `extra`. The item of a section that is only that item is never deleted.
Item {
  id: page

  // What kind of item it is, as read: "model", "skill", ...
  required property string kind
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
  // The id of its row, which it shows: "+" for a new item not saved yet.
  property string key: ""
  // The id of its row as the draft names it, which the table follows once
  // saved; "" keeps `key`.
  property string nextKey: ""

  // Wide enough for what is related to stand beside what it is.
  readonly property bool wide: scroll.width >= 640

  signal back()
  signal removing()
  signal followed(int section, string id)

  readonly property var commands: editing ? [
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
  ].concat(extra).concat([
    { text: I18n.t("settings.delete"), icon: "trash", role: "guarded", tip: removable ? I18n.t("settings.delete_tip") : keptReason,
      shown: !single && !fixed, enabled: removable, run: () => page.removing() }
  ])

  // Gives the keyboard to the screen: its first field when editing, else the
  // screen itself, whose Esc goes back.
  function takeFocus() {
    if (!editing) {
      page.forceActiveFocus()
      return
    }
    const first = formColumn.nextItemInFocusChain(true)
    if (first)
      first.forceActiveFocus()
  }
  function edit() {
    if (draft !== null)
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
  // Esc: out of editing, then back; a save under way keeps it. Returns whether
  // it took the key.
  function leave() {
    if (editing && draft !== null && draft.saving)
      return true
    if (editing)
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

      // What it is, and how far its edit is.
      Row {
        spacing: 12
        Label { text: page.kind.toUpperCase(); color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 3 }
        Label {
          visible: !!page.draft && page.draft.dirty && !page.draft.saving
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
        value: page.title || page.placeholder
        color: page.title ? Theme.foreground : Theme.dim
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
        visible: !page.editing
        columns: page.wide ? 2 : 1
        columnSpacing: 20
        rowSpacing: 20

        ColumnLayout {
          Layout.fillWidth: true
          Layout.alignment: Qt.AlignTop
          spacing: 20
          Frame {
            title: I18n.t("settings.about")
            Layout.fillWidth: true
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
          ColumnLayout {
            id: detailsColumn
            Layout.fillWidth: true
            visible: children.length > 0
            spacing: 12
          }
        }

        Frame {
          title: I18n.t("settings.related")
          Layout.preferredWidth: page.wide ? 300 : content.width
          Layout.maximumWidth: Layout.preferredWidth
          Layout.alignment: Qt.AlignTop
          Label {
            Layout.fillWidth: true
            visible: page.related.every(group => group.items.length === 0)
            text: I18n.t("settings.related_none")
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
          Repeater {
            model: page.related.filter(group => group.items.length > 0)
            delegate: ColumnLayout {
              id: group
              required property var modelData
              Layout.fillWidth: true
              spacing: 6
              Caption { text: group.modelData.title.toUpperCase() }
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
