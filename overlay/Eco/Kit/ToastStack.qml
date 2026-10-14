pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// ToastStack shows notices as toasts stacked upward from where it is anchored,
// the newest lowest. Each toast is its kind, its text and a × that closes it;
// Esc closes the toast that has the keyboard. Information and success close
// themselves after `timeout`, counted again from the start once the pointer
// leaves it and it loses the keyboard; errors stay until closed. At most `max`
// show: a newer one pushes the oldest out, unless it is pinned. Taller than
// `maxHeight`, the stack hides the oldest until they fit again, the pinned
// last. Toasts move only when they come and go.
ListView {
  id: stack

  // The notices, oldest first: {id, kind, parts, pinned}, `parts` as
  // {key, args} to translate or {text} as given.
  property var notices: []
  property int max: 3
  property int timeout: 6000
  property real maxHeight: Infinity
  // The height of each notice's toast, by id.
  property var heights: ({})
  // The notices whose toasts fit in `maxHeight`: the pinned, then the newest
  // first; the first always.
  readonly property var fitting: {
    const order = notices.filter(notice => notice.pinned).concat(notices.filter(notice => !notice.pinned).reverse())
    const ids = []
    let used = 0
    for (const notice of order) {
      used += (heights[notice.id] || 0) + (ids.length > 0 ? gap : 0)
      if (ids.length > 0 && used > maxHeight)
        break
      ids.push(notice.id)
    }
    return ids
  }
  // The room between two toasts.
  readonly property int gap: 8
  // The notice `id` is closed, by the user, its time, or one newer.
  signal dismissed(int id)

  width: 340
  height: contentHeight
  interactive: false
  model: ListModel { id: shown }

  // The toasts follow the notices: one gone leaves, one new comes.
  onNoticesChanged: sync()
  Component.onCompleted: sync()
  function sync() {
    const ids = notices.map(notice => notice.id)
    for (let i = shown.count - 1; i >= 0; i--)
      if (!ids.includes(shown.get(i).noticeId))
        shown.remove(i)
    const heights = {}
    for (const id of ids)
      if (stack.heights[id] !== undefined)
        heights[id] = stack.heights[id]
    stack.heights = heights
    const held = []
    for (let i = 0; i < shown.count; i++)
      held.push(shown.get(i).noticeId)
    for (const notice of notices)
      if (!held.includes(notice.id))
        shown.append({ noticeId: notice.id })
    const pushed = notices.filter(notice => !notice.pinned).slice(0, Math.max(0, notices.length - max))
    if (pushed.length > 0)
      Qt.callLater(() => pushed.forEach(notice => stack.dismissed(notice.id)))
  }

  add: Transition {
    NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 180; easing.type: Easing.OutCubic }
    NumberAnimation { property: "x"; from: 24; to: 0; duration: 180; easing.type: Easing.OutCubic }
  }
  remove: Transition {
    NumberAnimation { property: "opacity"; to: 0; duration: 140 }
  }
  displaced: Transition {
    NumberAnimation { property: "y"; duration: 180; easing.type: Easing.OutCubic }
  }

  delegate: FocusScope {
    id: toast

    required property int noticeId
    required property int index
    readonly property var notice: stack.notices.find(item => item.id === toast.noticeId) || null
    // The notice shown, kept while it leaves.
    property var kept: null
    Component.onCompleted: {
      kept = notice
      Qt.callLater(measured)
    }
    onNoticeChanged: if (notice !== null) kept = notice
    readonly property bool held: hover.hovered || toast.activeFocus
    // Whether it fits in the stack's height; a toast leaving keeps the last.
    property bool fits: true
    Binding on fits {
      when: toast.notice !== null
      value: stack.fitting.includes(toast.noticeId)
      restoreMode: Binding.RestoreNone
    }
    readonly property real cardHeight: card.implicitHeight
    // Told once the stack is laid out, which the heights lay out again.
    onCardHeightChanged: Qt.callLater(measured)
    function measured() {
      if (stack.heights[noticeId] !== cardHeight && notice !== null)
        stack.heights = Object.assign({}, stack.heights, { [noticeId]: cardHeight })
    }

    width: stack.width
    // The gap above each toast but the first.
    height: fits ? cardHeight + (index > 0 ? stack.gap : 0) : 0
    visible: fits
    Keys.onEscapePressed: stack.dismissed(toast.noticeId)

    // Clicks on a toast stay on it.
    MouseArea { anchors.fill: parent; acceptedButtons: Qt.AllButtons }
    HoverHandler { id: hover }
    Timer {
      interval: stack.timeout
      running: toast.notice !== null && toast.notice.kind !== "error" && !toast.held
      onTriggered: stack.dismissed(toast.noticeId)
    }

    // The toast's tint over the window's background, not over what it covers.
    Rectangle {
      anchors.fill: card
      color: Theme.background
    }
    StatusMessage {
      id: card
      anchors.bottom: parent.bottom
      width: parent.width
      kind: toast.kept === null ? "info" : toast.kept.kind
      message: toast.kept === null ? "" : Eco.noticeText(toast.kept)
      IconButton {
        name: "close"
        size: 10
        implicitWidth: 26
        implicitHeight: 26
        tip: I18n.t("toast.close_tip")
        onClicked: stack.dismissed(toast.noticeId)
      }
    }
  }
}
