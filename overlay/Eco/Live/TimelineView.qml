pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TimelineView is a session's conversation and, between its lines, the notes
// the user wrote and the suggestions they asked for, live or read back. Like a
// chat, it sits on the bottom and grows upwards, and it follows new lines
// unless the user scrolled up to read; then a pill counts what arrived below
// and jumps back to it. An answer streaming taller than the view keeps its top
// in view until the next entry comes. Below the lines, dimmed, what each
// speaker is saying right now, until it becomes a line. A click on a line, or
// Enter or the Menu key on it, opens its menu with everything that can be done
// to it; merging keeps that line outlined until the next line picked merges
// into it, and Esc, or the same line again, gives up. From the keyboard, Tab
// reaches it on the newest entry: ↑/↓ and PgUp/PgDn move, Home/End go to
// either end, N names the speaker of a line, and Tab moves into the tools of
// a note or an answer. A long session comes in pages: nearing either end of
// what it holds asks for the next, and a page above keeps the reading place.
Item {
  id: root

  property alias model: timeline.model
  // The session on screen, so a speaker can be named in it.
  property string sessionId
  onSessionIdChanged: {
    root.merging = null
    if (sessionId)
      Eco.requestSpeakers(sessionId)
  }
  // A model translates the session on screen, so its answers can be translated.
  readonly property bool translates: Eco.onScreen !== null && Eco.modelsOf(Eco.onScreen.kind).translation !== ""
  // [{who, name, text}]: words still being said.
  property var partials: []
  property string emptyText: Eco.recording ? I18n.t("timeline.listening") : I18n.t("timeline.paused")
  // Counts changes to the lines, so each knows again whether it continues the
  // turn before it: a line can be relabelled, added or removed under it.
  property int revision: 0

  // Entries that arrived while the view was not following, counted once each.
  property int unseen: 0
  property var seen: ({})
  // The answers streaming, by key, so the one that finishes counts as new.
  property var streams: ({})
  // The streaming answer whose top the view holds, and the one the user
  // jumped to the end of, which it does not hold.
  property string held: ""
  property string released: ""

  // Counts a new entry when the view is away from the end; speech has no key.
  function arrived(key) {
    if (timeline.following || (key !== "" && root.seen[key]))
      return
    if (key !== "")
      root.seen[key] = true
    root.unseen++
  }

  // Goes to the end and follows it again, past an answer still streaming; the
  // newest entries come back first when older ones were read.
  function jump() {
    if (Eco.moreAfter) {
      Eco.pageNewest()
      return
    }
    const newest = timeline.count > 0 ? timeline.model.get(timeline.count - 1) : null
    root.released = newest && newest.streaming ? newest.key : ""
    root.held = ""
    timeline.following = true
    if (timeline.currentIndex >= 0)
      timeline.currentIndex = timeline.count - 1
    timeline.positionViewAtEnd()
  }

  Connections {
    target: timeline.model
    // Only a speech line changing can change who continues whom; an answer
    // growing as it streams cannot, but it is new again when it finishes.
    function onDataChanged(topLeft) {
      const entry = timeline.model.get(topLeft.row)
      if (entry.kind === "speech")
        root.revision++
      else if (entry.streaming)
        root.streams[entry.key] = true
      else if (root.streams[entry.key]) {
        delete root.streams[entry.key]
        root.arrived(entry.key)
      }
    }
    function onRowsAboutToBeInserted(parent, first) {
      if (first === 0 && timeline.count > 0)
        root.hold()
    }
    function onRowsInserted(parent, first, last) {
      root.revision++
      for (let row = first; row <= last; row++) {
        const entry = timeline.model.get(row)
        if (entry.streaming)
          root.streams[entry.key] = true
        if (entry.fresh)
          root.arrived(entry.key)
      }
      if (root.anchor >= 0)
        root.keep(last - first + 1)
    }
    // A cleared timeline is another one loading: it starts at its end.
    function onRowsRemoved() {
      root.revision++
      if (timeline.count === 0) {
        timeline.following = true
        root.streams = {}
        root.held = ""
        root.released = ""
      }
    }
  }

  // The keyboard is on the timeline or in the entry it is on.
  readonly property bool keyboard: timeline.activeFocus || (timeline.currentItem !== null && Focus.within(Window.activeFocusItem, timeline.currentItem))

  // Timeline pages (docs/design.md §10).
  // How near either end of what it holds, in entries, the view asks for more.
  readonly property int near: 20
  // The entry at the top of the view while a page goes in above it, and how
  // far below the view's top it was; -1 when none goes in.
  property int anchor: -1
  property real anchorOffset: 0

  // Asks for the entries beyond those held once the view, away from the
  // newest it follows, nears either end.
  function reach() {
    if (timeline.count === 0 || timeline.following)
      return
    const x = timeline.width / 2
    const top = timeline.indexAt(x, timeline.contentY + timeline.topMargin + 1)
    const bottom = timeline.indexAt(x, timeline.contentY + timeline.height - timeline.bottomMargin - 1)
    if (Eco.moreBefore && (timeline.atYBeginning || (top >= 0 && top < root.near)))
      Eco.pageBefore()
    else if (Eco.moreAfter && (timeline.atYEnd || bottom >= timeline.count - root.near))
      Eco.pageAfter()
  }

  // Notes the entry at the top of the view before a page goes in above it.
  function hold() {
    // Past the strip above the entries, or the spacing between two.
    let top = -1
    for (let y = timeline.contentY + timeline.topMargin + 1; top < 0 && y < timeline.contentY + timeline.height; y += 4)
      top = timeline.indexAt(timeline.width / 2, y)
    const item = top >= 0 ? timeline.itemAtIndex(top) : null
    root.anchor = item ? top : -1
    root.anchorOffset = item ? item.y - timeline.contentY : 0
  }

  // Puts the entry noted back where it was, `added` entries further down.
  function keep(added) {
    const index = root.anchor + added
    root.anchor = -1
    timeline.forceLayout()
    const item = timeline.itemAtIndex(index)
    if (item)
      timeline.contentY = item.y - root.anchorOffset
    else
      timeline.positionViewAtIndex(index, ListView.Beginning)
  }

  Connections {
    target: Eco
    // A jump from a search: the entry found shows in the middle of the view,
    // which stops following the newest.
    function onTimelineJumped(index) {
      timeline.following = false
      root.centre(index)
      // Again once the entries around it are laid out.
      Qt.callLater(root.centre, index)
    }
  }
  function centre(index) { timeline.positionViewAtIndex(index, ListView.Center) }
  // End of timeline pages.

  // Seconds of silence after which a speaker's next line starts a new turn.
  readonly property int pause: 120

  // >>> line menu (merge and split)
  // The line {who, at} waiting for the line it merges with, or null.
  property var merging: null
  function isMerging(who, at) { return root.merging !== null && root.merging.who === who && Math.abs(root.merging.at - at) < 1e-6 }
  // Everything that can be done to a line, as its menu lists it.
  readonly property var lineActions: [
    { id: "edit", icon: "edit", text: I18n.t("line.menu.edit") },
    { id: "line_speaker", icon: "people", text: I18n.t("line.menu.speaker_line") },
    { id: "speaker", icon: "people", text: I18n.t("line.menu.speaker_all") },
    { id: "copy", icon: "copy", text: I18n.t("line.menu.copy") },
    { id: "merge", icon: "merge", text: I18n.t("line.menu.merge") },
    { id: "split", icon: "split", text: I18n.t("line.menu.split") },
    { id: "remove", icon: "trash", text: I18n.t("line.menu.remove"), guarded: true }
  ]
  // A line clicked, or chosen from the keyboard, at `row`: its menu opens, or
  // it merges into the line waiting; the waiting line itself gives up.
  function lineClicked(turn, row) {
    if (root.merging === null) {
      lineMenu.line = turn
      lineMenu.row = row
      lineMenu.parent = turn.box
      lineMenu.alignRight = turn.mine
      lineMenu.open()
      return
    }
    const same = root.isMerging(turn.who, turn.at)
    const into = root.merging
    root.merging = null
    if (!same)
      Eco.mergeLines(root.sessionId, into, { who: turn.who, at: turn.at })
  }
  function act(id, turn, row) {
    const speaker = scope => Eco.speakerRequested({ session: root.sessionId, label: turn.who, name: turn.name, at: turn.at, scope: scope })
    if (id === "edit")
      turn.edit()
    else if (id === "line_speaker")
      speaker("line")
    else if (id === "speaker")
      speaker("speaker")
    else if (id === "copy")
      Eco.copy(turn.text)
    else if (id === "merge") {
      root.merging = { who: turn.who, at: turn.at }
      // The keyboard goes to the timeline, on the line waiting, so Esc gives up.
      timeline.currentIndex = row
      timeline.forceActiveFocus()
    } else if (id === "split")
      turn.split()
    else if (id === "remove")
      Eco.removeLine(root.sessionId, turn.who, turn.at)
  }

  MenuPopup {
    id: lineMenu
    // The line it was opened on, and its row.
    property var line: null
    property int row: -1
    entries: root.lineActions
    onPicked: index => root.act(root.lineActions[index].id, lineMenu.line, lineMenu.row)
  }

  // While a line waits for its merge: what to do, and a way out.
  RowLayout {
    id: mergeStrip
    visible: root.merging !== null
    anchors { left: parent.left; right: parent.right; top: parent.top }
    spacing: 10
    Icon { name: "merge"; size: 11; color: Theme.primary }
    Label {
      Layout.fillWidth: true
      text: I18n.t("line.merge_hint")
      color: Theme.primary
      font.pixelSize: 10
      font.letterSpacing: 2
      elide: Text.ElideRight
    }
    Chip { text: I18n.t("dialog.cancel"); onClicked: root.merging = null }
  }
  // <<< line menu

  // Whether the line at `index` is said by the speaker of the line just before
  // it, with no longer pause between them.
  // A delegate may ask with its old index while the lines change under it.
  function continues(index) {
    if (index <= 0 || index >= timeline.model.count)
      return false
    const previous = timeline.model.get(index - 1)
    const current = timeline.model.get(index)
    return previous.kind === "speech" && current.kind === "speech" && previous.who === current.who
      && current.at - previous.at < root.pause
  }

  ListView {
  id: timeline

  property bool following: true
  onFollowingChanged: {
    Eco.timelineFollowing = following
    if (following) {
      root.unseen = 0
      root.seen = {}
      Eco.capTimeline()
    }
  }

  // Keeps the newest entry in view: its end, or the top of an answer
  // streaming taller than the view.
  function follow() {
    if (!following)
      return
    const last = count - 1
    const entry = itemAtIndex(last)
    if (entry && entry.streaming && entry.key !== root.released && entry.height > root.height - topMargin - bottomMargin)
      root.held = entry.key
    if (entry && entry.key !== "" && entry.key === root.held)
      positionViewAtIndex(last, ListView.Beginning)
    else
      positionViewAtEnd()
  }

  anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
  height: Math.min(root.height - (mergeStrip.visible ? mergeStrip.height + 8 : 0), contentHeight + topMargin + bottomMargin)
  clip: true
  // An entry's outline never lies on the clip's edge, where a fractional scale
  // would cut it.
  topMargin: 1
  bottomMargin: 1
  spacing: 4
  model: Eco.timeline
  // Only the session's own end is followed, not the end of a page held.
  onMovementEnded: {
    following = atYEnd && !Eco.moreAfter
    root.reach()
  }
  onContentYChanged: Qt.callLater(root.reach)
  Component.onCompleted: Eco.timelineFollowing = following
  onContentHeightChanged: if (following) Qt.callLater(follow)
  onCountChanged: if (following) Qt.callLater(follow)
  // A strip appearing above takes room from the view, never the newest entry.
  onHeightChanged: if (following) Qt.callLater(follow)

  // No entry is current until the keyboard comes, so nothing pulls the view.
  currentIndex: -1
  activeFocusOnTab: true
  keyNavigationEnabled: true
  onActiveFocusChanged: if (activeFocus && (currentIndex < 0 || currentIndex >= count)) currentIndex = count - 1
  onCurrentIndexChanged: following = currentIndex === count - 1 && !Eco.moreAfter
  Keys.onPressed: event => {
    const page = { [Qt.Key_PageDown]: 5, [Qt.Key_PageUp]: -5 }[event.key]
    if (page !== undefined)
      currentIndex = Math.max(0, Math.min(count - 1, currentIndex + page))
    else if (event.key === Qt.Key_Home)
      currentIndex = 0
    else if (event.key === Qt.Key_End)
      root.jump()
    else if (event.key === Qt.Key_N && currentItem && currentItem.kind === "speech" && root.sessionId)
      Eco.speakerRequested({ session: root.sessionId, label: currentItem.who, name: currentItem.name, at: currentItem.at, scope: "speaker" })
    else if ([Qt.Key_Return, Qt.Key_Enter, Qt.Key_Menu].includes(event.key) && currentItem && currentItem.kind === "speech" && root.sessionId)
      root.lineClicked(currentItem.item, currentIndex)
    else if (event.key === Qt.Key_Escape && root.merging !== null)
      root.merging = null
    else
      return
    event.accepted = true
  }

  // Above what it holds, the earlier entries a scroll up brings.
  header: Item {
    width: timeline.width
    height: Eco.moreBefore ? 28 : 0
    visible: Eco.moreBefore
    Caption {
      anchors.centerIn: parent
      text: I18n.t("timeline.earlier")
    }
    LoadingTrace {
      anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
      height: 2
      running: Eco.timelineAsked !== null && Eco.timelineAsked.before !== null
    }
  }

  footer: Column {
    width: timeline.width
    topPadding: root.partials.length > 0 ? 8 : 0
    spacing: 8
    Repeater {
      model: root.partials
      delegate: SpeechTurn {
        required property var modelData
        width: timeline.width
        who: modelData.who
        name: modelData.name || modelData.who
        sessionId: root.sessionId
        text: modelData.text
        mine: Eco.isUser(modelData.who)
        stamp: ""
        at: 0
        partial: true
      }
    }
  }

  // Removed lines fade; what they leave behind closes up instead of jumping.
  // Entries put in above, a page read back, move nothing.
  remove: Transition {
    NumberAnimation { property: "opacity"; to: 0; duration: 140 }
  }
  removeDisplaced: Transition {
    NumberAnimation { property: "y"; duration: 180; easing.type: Easing.OutCubic }
  }

  delegate: Loader {
    id: entry

    required property int index
    required property string kind
    required property string key
    required property string who
    required property string name
    required property string text
    required property bool mine
    required property string stamp
    required property real at
    required property string action
    required property string modelName
    required property string prompt
    required property bool streaming
    required property string thinking
    required property string failure
    required property string draft
    required property string hook
    required property string translation
    required property string timing
    required property bool fresh

    width: timeline.width
    sourceComponent: kind === "speech" ? speech : kind === "note" ? note : suggestion
    readonly property bool selected: ListView.isCurrentItem && root.keyboard

    FocusRing { shown: entry.selected; anchors.margins: 0 }

    // A new line slides in from its speaker's side; answers rise from below.
    transform: Translate { id: shift }
    ListView.onAdd: if (entry.fresh) arrive.start()
    ParallelAnimation {
      id: arrive
      NumberAnimation { target: entry; property: "opacity"; from: 0; to: 1; duration: 260; easing.type: Easing.OutCubic }
      NumberAnimation {
        target: shift
        property: entry.kind === "speech" ? "x" : "y"
        from: entry.kind === "speech" ? (entry.mine ? 28 : -28) : 16
        to: 0
        duration: 300
        easing.type: Easing.OutCubic
      }
    }

    Component {
      id: speech
      SpeechTurn {
        width: entry.width
        who: entry.who
        name: entry.name
        sessionId: root.sessionId
        text: entry.text
        mine: entry.mine
        stamp: entry.stamp
        at: entry.at
        continued: root.revision >= 0 && root.continues(entry.index)
        selected: entry.selected
        translation: entry.translation
        merging: root.isMerging(entry.who, entry.at)
        onClicked: root.lineClicked(this, entry.index)
      }
    }

    Component {
      id: note
      NoteCard {
        width: entry.width
        noteId: entry.key
        text: entry.text
        stamp: entry.stamp
        reachable: entry.selected
      }
    }

    Component {
      id: suggestion
      SuggestionCard {
        width: entry.width
        suggestionId: entry.key
        text: entry.text
        action: entry.action
        modelName: entry.modelName
        prompt: entry.prompt
        streaming: entry.streaming
        thinking: entry.thinking
        failure: entry.failure
        draft: entry.draft
        hookable: Eco.hooks.includes(entry.action)
        hookState: entry.hook
        translation: entry.translation
        translatable: root.translates
        timing: entry.timing
        at: entry.at
        stamp: entry.stamp
        reachable: entry.selected
      }
    }
  }

  }

  NewPill {
    anchors { horizontalCenter: parent.horizontalCenter; bottom: parent.bottom; bottomMargin: 10 }
    count: root.unseen
    onClicked: root.jump()
  }

  Label {
    anchors.centerIn: parent
    visible: timeline.count === 0 && root.partials.length === 0
    text: root.emptyText
    color: Theme.line
    font.pixelSize: 11
    font.letterSpacing: 3
  }
}
