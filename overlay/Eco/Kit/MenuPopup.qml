pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// MenuPopup is the list a menu button opens below itself: one ChoiceRow per entry ({text, icon, chosen, guarded}), `picked` giving its
// index; an entry {heading} is a group's title, never picked nor stepped on.
// As wide as its longest entry and at least `minimumWidth`, aligned to
// the button's right edge when `alignRight`, above it when `above`; long lists
// scroll. The keyboard starts on the chosen entry: Up/Down/Home/End move, Enter
// or Space picks. A completion list leaves the keyboard in its field
// (`takesKeyboard: false`), which drives it with move() and pick().
PopupFrame {
  id: menu

  property var entries: []
  property real minimumWidth: 0
  property bool alignRight: false
  property bool above: false
  property bool takesKeyboard: true
  signal picked(int index)
  // The entry the keyboard is on.
  property int current: 0

  function pick(index) {
    menu.close()
    menu.picked(index)
  }
  // Moves the keyboard to `index`, or past headings on in the direction of the
  // move, or back when none follows.
  function move(index) {
    const step = index < current ? -1 : 1
    let to = Math.max(0, Math.min(entries.length - 1, index))
    while (entries[to] && entries[to].heading !== undefined && entries[to + step])
      to += step
    while (entries[to] && entries[to].heading !== undefined && entries[to - step])
      to -= step
    current = to
    const row = rows.itemAt(current)
    if (row)
      scroll.contentY = Math.max(Math.min(scroll.contentY, row.y - scroll.topMargin), row.y + row.height + scroll.bottomMargin - scroll.height)
  }

  TextMetrics {
    id: metrics
    font.family: Theme.fontFamily
    font.pixelSize: 11
    font.letterSpacing: 1
  }

  focus: takesKeyboard
  y: above ? -height - 4 : parent.height + 4
  padding: 4
  // Measured when it opens, and again when its entries change while shown:
  // measuring inside a binding would loop.
  function measure() {
    let widest = 0
    for (const entry of entries) {
      metrics.text = entry.heading !== undefined ? entry.heading : entry.text
      widest = Math.max(widest, metrics.advanceWidth)
    }
    width = Math.max(minimumWidth, widest + (entries.some(entry => entry.icon) ? 50 : 42))
    x = alignRight ? parent.width - width : 0
  }
  // The keyboard goes to the chosen entry, or stays where it is within the list.
  function place() {
    const chosen = entries.findIndex(entry => entry.chosen)
    move(chosen >= 0 ? chosen : Math.min(current, entries.length - 1))
  }
  onAboutToShow: {
    measure()
    current = 0
    place()
    scroll.contentY = -scroll.topMargin
  }
  onEntriesChanged: if (visible) {
    measure()
    place()
  }
  height: Math.min(column.implicitHeight + 10, 320)

  Flickable {
    id: scroll
    anchors.fill: parent
    clip: true
    focus: true
    contentHeight: column.implicitHeight
    // A row's outline never lies on the clip's edge, where a fractional scale
    // would cut it.
    topMargin: 1
    bottomMargin: 1
    Keys.onPressed: event => {
      const step = { [Qt.Key_Down]: 1, [Qt.Key_Up]: -1, [Qt.Key_PageDown]: 8, [Qt.Key_PageUp]: -8 }[event.key]
      if (step !== undefined)
        menu.move(menu.current + step)
      else if (event.key === Qt.Key_Home)
        menu.move(0)
      else if (event.key === Qt.Key_End)
        menu.move(menu.entries.length - 1)
      else if ([Qt.Key_Return, Qt.Key_Enter, Qt.Key_Space].includes(event.key) && menu.entries[menu.current] && menu.entries[menu.current].heading === undefined)
        menu.pick(menu.current)
      else
        return
      event.accepted = true
    }
    Column {
      id: column
      width: parent.width
      Repeater {
        id: rows
        model: menu.entries
        delegate: Loader {
          id: entry
          required property var modelData
          required property int index
          width: column.width
          sourceComponent: modelData.heading !== undefined ? heading : choice
          Component {
            id: heading
            Caption {
              topPadding: entry.index > 0 ? 10 : 4
              bottomPadding: 4
              leftPadding: 11
              text: entry.modelData.heading
            }
          }
          Component {
            id: choice
            ChoiceRow {
              text: entry.modelData.text
              icon: entry.modelData.icon || ""
              chosen: !!entry.modelData.chosen
              guarded: !!entry.modelData.guarded
              current: entry.index === menu.current
              onPicked: menu.pick(entry.index)
            }
          }
        }
      }
    }
  }
}
