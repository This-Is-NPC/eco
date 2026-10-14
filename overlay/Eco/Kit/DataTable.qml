pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Controls.Basic as C
import Eco.Core

// DataTable lists records as rows of columns under a header; below 700 px each
// row stacks its cells into lines instead. A row opens on click, Enter or
// Space, or, when the table `picks`, a click picks it and a double click opens
// it; ↑/↓ move, ↑ on the first asks to leave upward, Delete asks to remove it,
// Esc asks to go back. Each row may carry tools on its right, from `tools` (its
// root gets the row's record as `entry`): Tab reaches those of the selected row
// only, or, `toolsLive`, every row's, row by row. Tools whose root has `act()`
// take the row's Enter, Space and double click instead of opening it; Esc, or
// Enter a tool leaves, goes back to the rows. Rows may also carry a status
// cell, and be listed under group titles.
Item {
  id: table

  // {title, weight}: the weights of all columns add up to 1.
  property var columns: []
  property var rows: []
  // The cell texts of a record, one per column.
  property var cells: entry => []
  // A cell's colour, or "" for the usual one (the first column reads brighter).
  property var tone: (entry, column) => ""
  // The columns shown on each line of a stacked row.
  property var stacked: [[0]]
  // The column shown at the top right of a stacked row, or -1.
  property int status: -1
  // A record whose status shows a dot before it, as one going on now.
  property var dotted: entry => false
  // The field of a record naming the group it is listed under, or "" for none:
  // a title opens each run of records sharing it.
  property string section: ""
  property Component tools
  property real toolsWidth: 0
  // The title over the tools in the header, or "".
  property string toolsTitle: ""
  // The tools stay on every row however narrow the table.
  property bool toolsAlways: false
  // Every row's tools work, as values do, rather than the selected row's
  // only: Tab from the rows reaches the selected row's, and walks on row by
  // row, picking each row it enters.
  property bool toolsLive: false
  // A record that steps back, as when it cannot be picked.
  property var dimmed: entry => false
  property var idOf: entry => entry.id
  property string selectedId: ""
  // Whether a click picks a row rather than opening it.
  property bool picks: false
  property string emptyText
  // What the empty list offers to do about it, or "", and its icon.
  property string emptyAction
  property string emptyIcon: "close"
  // Whether the records are still to come: the empty list shows it loads.
  property bool loading: false
  property bool active: false
  readonly property bool compact: width < 700
  // Below this a row shows its tools only when pointed at or selected, so its
  // text keeps the room.
  readonly property bool narrow: width < 360
  readonly property int headerHeight: compact ? 0 : 32
  // The ids of the records listed last, so a change decodes only the new ones.
  property var listed: new Set()
  // The record the keyboard is on, or null.
  readonly property var currentEntry: list.currentItem ? list.currentItem.modelData : null

  signal opened(var entry)
  signal picked(var entry)
  signal removeRequested(var entry)
  signal backRequested()
  signal topLeft()
  signal emptyActed()

  implicitHeight: headerHeight + (compact ? 0 : 10) + list.topMargin + list.bottomMargin + Math.max(52, list.contentHeight)
  onActiveChanged: if (active) list.forceActiveFocus()
  onRowsChanged: {
    const before = listed
    listed = new Set(rows.map(entry => idOf(entry)))
    if (active)
      Qt.callLater(() => table.reveal(before))
    Qt.callLater(table.follow)
  }
  onSelectedIdChanged: follow()

  // Whether the keyboard was last on the rows or a row's tools, a list or a
  // dialog they opened in between.
  property bool held: false
  readonly property Item overlay: C.Overlay.overlay
  // New rows destroy the row the keyboard was on, which leaves the window
  // with none: the rows take it back.
  Connections {
    target: table.Window.window
    function onActiveFocusItemChanged() {
      const item = table.Window.activeFocusItem
      if (table.held && table.visible && (item === null || item === table.Window.contentItem)) {
        list.forceActiveFocus()
        return
      }
      let at = item
      while (at !== null && at !== list && at !== table.overlay)
        at = at.parent
      if (at !== table.overlay)
        table.held = at !== null
    }
  }

  // Picking, the keyboard's row is the picked one, wherever the rows put it.
  function follow() {
    const at = picks && selectedId !== "" ? rows.findIndex(entry => idOf(entry) === selectedId) : -1
    if (at >= 0)
      list.currentIndex = at
  }
  // The keyboard moves to the row `step` away, which, picking, is picked.
  function step(by) {
    list.currentIndex += by
    if (picks && list.currentItem)
      picked(list.currentItem.modelData)
  }

  // Decode the rows that were not listed `before`.
  function reveal(before) {
    for (let i = 0; i < list.count; i++) {
      const item = list.itemAtIndex(i)
      if (item && !before.has(idOf(item.modelData)))
        item.reveal()
    }
  }

  // Give the keyboard to the rows.
  function focusRows() { list.forceActiveFocus() }

  // A column the columns lost before the rows were rebuilt takes no room.
  function columnWidth(index) {
    return index < columns.length ? Math.max(0, width - 24 - toolsWidth) * columns[index].weight : 0
  }

  Rectangle {
    id: header
    visible: !table.compact
    width: parent.width
    height: table.headerHeight
    color: Qt.alpha(Theme.primary, 0.06)
    border.color: Theme.line
    Row {
      anchors { fill: parent; leftMargin: 12; rightMargin: 12 }
      Repeater {
        model: table.columns
        delegate: Label {
          required property var modelData
          required property int index
          width: table.columnWidth(index)
          height: header.height
          verticalAlignment: Text.AlignVCenter
          text: modelData.title.toUpperCase()
          color: Theme.dim
          font.pixelSize: 9
          font.letterSpacing: 1
          elide: Text.ElideRight
        }
      }
      Label {
        visible: table.toolsTitle !== ""
        width: table.toolsWidth
        height: header.height
        verticalAlignment: Text.AlignVCenter
        text: table.toolsTitle.toUpperCase()
        color: Theme.dim
        font.pixelSize: 9
        font.letterSpacing: 1
        elide: Text.ElideRight
      }
    }
  }

  ListView {
    id: list
    anchors { left: parent.left; right: parent.right; top: header.bottom; bottom: parent.bottom; topMargin: table.compact ? 0 : 10 }
    clip: true
    // A row's outline never lies on the clip's edge, where a fractional scale
    // would cut it.
    topMargin: 1
    bottomMargin: 1
    spacing: 2
    model: table.rows
    section.property: table.section
    section.delegate: Kicker {
      required property string section
      width: ListView.view.width
      height: 26
      text: section
    }
    activeFocusOnTab: true
    onActiveFocusChanged: {
      if (activeFocus && currentIndex < 0 && count > 0)
        currentIndex = 0
      // The row the keyboard lands on is the one picked.
      if (activeFocus && table.picks && currentItem)
        table.picked(currentItem.modelData)
    }
    Keys.onReturnPressed: if (currentItem) currentItem.act()
    Keys.onSpacePressed: if (currentItem) currentItem.act()
    Keys.onDeletePressed: if (currentItem) table.removeRequested(currentItem.modelData)
    Keys.onTabPressed: event => event.accepted = table.toolsLive && currentItem !== null && currentItem.enterTools()
    Keys.onEscapePressed: table.backRequested()
    Keys.onDownPressed: if (currentIndex < count - 1) table.step(1)
    Keys.onUpPressed: {
      if (currentIndex > 0) {
        table.step(-1)
        return
      }
      positionViewAtBeginning()
      table.topLeft()
    }

    delegate: Rectangle {
      id: row
      required property var modelData
      required property int index
      readonly property var texts: table.cells(modelData)
      readonly property bool dot: table.dotted(modelData)
      // The stacked lines that hold text: the title's always.
      readonly property int lineCount: table.stacked.filter((line, i) => i === 0 || line.some(column => texts[column])).length
      readonly property bool current: ListView.isCurrentItem
      readonly property bool marked: table.selectedId !== "" && table.selectedId === table.idOf(modelData) || (current && (list.activeFocus || toolScope.activeFocus))
      readonly property bool tooled: table.tools !== null && (!table.narrow || table.toolsAlways || hover.hovered || marked)
      readonly property real reserved: tooled ? table.toolsWidth : 0

      function open() {
        list.currentIndex = index
        table.opened(modelData)
      }
      // Enter, Space or a double click: the tools' own act, else opening it.
      function act() {
        if (tools.item && typeof tools.item.act === "function") {
          list.currentIndex = index
          tools.item.act()
        } else {
          open()
        }
      }
      // Gives the keyboard to the first of its tools; false when it has none.
      function enterTools() {
        const first = toolScope.nextItemInFocusChain(true)
        if (!toolScope.visible || first === null || !isWithin(first))
          return false
        first.forceActiveFocus(Qt.TabFocusReason)
        return true
      }
      function isWithin(item) {
        for (let at = item; at !== null; at = at.parent) {
          if (at === toolScope)
            return true
        }
        return false
      }
      function pick() {
        list.currentIndex = index
        list.forceActiveFocus()
        table.picked(modelData)
      }

      function color(column) {
        return table.tone(modelData, column) || (column === 0 ? Theme.foreground : Theme.dim)
      }

      function reveal() {
        const cells = table.compact ? lines : wide
        for (let i = 0; i < cells.count; i++) {
          const cell = cells.itemAt(i)
          if (cell)
            cell.replay()
        }
      }

      width: list.width
      height: table.compact ? 22 + lineCount * 20 : 52
      opacity: table.dimmed(modelData) ? 0.4 : 1
      color: marked ? Theme.highlight : pointer.containsMouse ? Qt.alpha(Theme.primary, 0.035) : "transparent"
      border.color: marked ? Theme.primary : Theme.line
      Behavior on color { ColorAnimation { duration: 120 } }
      Behavior on opacity { NumberAnimation { duration: 140 } }

      HoverHandler { id: hover }

      Row {
        visible: !table.compact
        anchors { fill: parent; leftMargin: 12; rightMargin: 12 + row.reserved }
        Repeater {
          id: wide
          model: row.texts
          delegate: DecodeLabel {
            required property string modelData
            required property int index
            width: table.columnWidth(index)
            height: row.height
            verticalAlignment: Text.AlignVCenter
            value: modelData
            soft: true
            revealOnStage: !table.compact
            leftPadding: index === table.status && row.dot ? 11 : 0
            color: row.color(index)
            font.pixelSize: index === 0 ? 11 : 10
            elide: Text.ElideRight
            Dot {
              visible: parent.leftPadding > 0
              anchors.verticalCenter: parent.verticalCenter
              tone: parent.color
            }
          }
        }
      }

      Column {
        id: stack
        visible: table.compact
        anchors { left: parent.left; right: statusLabel.left; top: parent.top; leftMargin: 12; rightMargin: 10; topMargin: 11 }
        spacing: 5
        Repeater {
          id: lines
          model: table.stacked
          delegate: DecodeLabel {
            required property var modelData
            required property int index
            width: stack.width
            value: modelData.map(column => row.texts[column]).filter(text => text).join("  ·  ")
            // A line with nothing to say takes no room, but the title's.
            visible: index === 0 || value !== ""
            soft: true
            revealOnStage: table.compact
            color: row.color(modelData[0])
            font.pixelSize: index === 0 ? 12 : 10
            elide: Text.ElideRight
          }
        }
      }

      Label {
        id: statusLabel
        readonly property bool shown: table.compact && table.status >= 0
        visible: shown
        anchors { right: parent.right; rightMargin: 12 + row.reserved; top: parent.top; topMargin: 12 }
        // Not `visible`, which also follows the row's: a row torn down would loop.
        width: shown ? implicitWidth : 0
        leftPadding: row.dot ? 11 : 0
        text: table.status >= 0 ? row.texts[table.status] : ""
        color: table.tone(row.modelData, table.status) || Theme.primary
        font.pixelSize: 9
        font.letterSpacing: 1
        Dot {
          visible: row.dot
          anchors.verticalCenter: parent.verticalCenter
          tone: parent.color
        }
      }

      MouseArea {
        id: pointer
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        onClicked: table.picks ? row.pick() : row.open()
        onDoubleClicked: if (table.picks) row.act()
      }

      // Only the selected row's tools are Tab stops, unless every row's are
      // live; the pointer reaches any.
      FocusScope {
        id: toolScope
        visible: row.tooled
        enabled: table.toolsLive || row.current || hover.hovered
        // A live tool the keyboard or the pointer reaches picks its row.
        onActiveFocusChanged: {
          if (activeFocus && table.toolsLive && !row.current) {
            // The list hands the keyboard to its new current row: give it back.
            const tool = Window.activeFocusItem
            list.currentIndex = row.index
            if (table.picks)
              table.picked(row.modelData)
            tool.forceActiveFocus(Qt.TabFocusReason)
          }
        }
        anchors { right: parent.right; rightMargin: 6; verticalCenter: parent.verticalCenter }
        width: tools.width
        height: tools.height
        Loader {
          id: tools
          sourceComponent: table.tools
          onLoaded: item.entry = Qt.binding(() => row.modelData)
        }
        // Esc, and Enter a tool leaves, as a field confirming what was typed,
        // give the keyboard back to the rows: the tool lets it go first, or
        // the list would hand it straight back.
        function leave() {
          focus = false
          list.forceActiveFocus()
        }
        Keys.onEscapePressed: leave()
        Keys.onReturnPressed: leave()
        Keys.onEnterPressed: leave()
      }
    }
  }

  // Beside the list, not in it: a ListView's children scroll with its empty content.
  Column {
    anchors { left: list.left; right: list.right; top: list.top; margins: 20 }
    visible: list.count === 0
    spacing: 14
    Label {
      width: parent.width
      horizontalAlignment: Text.AlignHCenter
      text: table.emptyText
      color: Theme.dim
      font.pixelSize: 11
      wrapMode: Text.Wrap
    }
    LoadingTrace {
      anchors.horizontalCenter: parent.horizontalCenter
      width: 120
      height: 16
      running: table.loading
    }
    TraceButton {
      anchors.horizontalCenter: parent.horizontalCenter
      visible: table.emptyAction !== ""
      dense: true
      role: "action"
      icon: table.emptyIcon
      text: table.emptyAction
      onClicked: table.emptyActed()
    }
  }
}
