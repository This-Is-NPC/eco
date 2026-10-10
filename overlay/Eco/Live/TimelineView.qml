pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// TimelineView is a session's conversation and, between its lines, the notes
// the user wrote and the suggestions they asked for, live or read back. Like a
// chat, it sits on the bottom and grows upwards, and it follows new lines
// unless the user scrolled up to read; then a pill counts what arrived below
// and jumps back to it. An answer streaming taller than the view keeps its top
// in view until the next entry comes. Below the lines, dimmed, what each
// speaker is saying right now, until it becomes a line. From the keyboard, Tab
// reaches it on the newest entry: ↑/↓ and PgUp/PgDn move, Home/End go to
// either end, N names the speaker of a line, and Tab moves into the tools of
// the entry it is on.
Item {
  id: root

  property alias model: timeline.model
  // The session on screen, so a speaker can be named in it.
  property string sessionId
  onSessionIdChanged: if (sessionId) Eco.requestSpeakers(sessionId)
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

  // Goes to the end and follows it again, past an answer still streaming.
  function jump() {
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
    function onRowsInserted(parent, first, last) {
      root.revision++
      for (let row = first; row <= last; row++) {
        const entry = timeline.model.get(row)
        if (entry.streaming)
          root.streams[entry.key] = true
        root.arrived(entry.key)
      }
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

  // Seconds of silence after which a speaker's next line starts a new turn.
  readonly property int pause: 120

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
  onFollowingChanged: if (following) { root.unseen = 0; root.seen = {} }

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
  height: Math.min(root.height, contentHeight + topMargin + bottomMargin)
  clip: true
  // An entry's outline never lies on the clip's edge, where a fractional scale
  // would cut it.
  topMargin: 1
  bottomMargin: 1
  spacing: 4
  model: Eco.timeline
  onMovementEnded: following = atYEnd
  onContentHeightChanged: if (following) Qt.callLater(follow)
  onCountChanged: if (following) Qt.callLater(follow)
  // A strip appearing above takes room from the view, never the newest entry.
  onHeightChanged: if (following) Qt.callLater(follow)

  // No entry is current until the keyboard comes, so nothing pulls the view.
  currentIndex: -1
  activeFocusOnTab: true
  keyNavigationEnabled: true
  onActiveFocusChanged: if (activeFocus && (currentIndex < 0 || currentIndex >= count)) currentIndex = count - 1
  onCurrentIndexChanged: following = currentIndex === count - 1
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
    else
      return
    event.accepted = true
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
  remove: Transition {
    NumberAnimation { property: "opacity"; to: 0; duration: 140 }
  }
  displaced: Transition {
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

    width: timeline.width
    sourceComponent: kind === "speech" ? speech : kind === "note" ? note : suggestion
    readonly property bool selected: ListView.isCurrentItem && root.keyboard

    FocusRing { shown: entry.selected; anchors.margins: 0 }

    // A new line slides in from its speaker's side; answers rise from below.
    transform: Translate { id: shift }
    ListView.onAdd: arrive.start()
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
