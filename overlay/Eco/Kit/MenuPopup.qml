pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core

// MenuPopup is the list a menu button opens below itself: one ChoiceRow per entry ({text, icon, chosen, guarded}), `picked` giving its
// index. As wide as its longest entry and at least `minimumWidth`, aligned to
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
  function move(index) {
    current = Math.max(0, Math.min(entries.length - 1, index))
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
  // Measured when it opens: measuring inside a binding would loop.
  onAboutToShow: {
    let widest = 0
    for (const entry of entries) {
      metrics.text = entry.text
      widest = Math.max(widest, metrics.advanceWidth)
    }
    width = Math.max(minimumWidth, widest + (entries.some(entry => entry.icon) ? 50 : 42))
    x = alignRight ? parent.width - width : 0
    current = Math.max(0, entries.findIndex(entry => entry.chosen))
    scroll.contentY = -scroll.topMargin
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
      else if ([Qt.Key_Return, Qt.Key_Enter, Qt.Key_Space].includes(event.key) && menu.entries.length > 0)
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
        delegate: ChoiceRow {
          required property var modelData
          required property int index
          width: column.width
          text: modelData.text
          icon: modelData.icon || ""
          chosen: !!modelData.chosen
          guarded: !!modelData.guarded
          current: index === menu.current
          onPicked: menu.pick(index)
        }
      }
    }
  }
}
