import QtQuick
import QtQuick.Layouts

// SessionView is a session on screen, live or stored, as one conversation: a
// live one under its capsule, a stored one under a line saying when it was and
// how long, with the way to resume it. On a short window the capsule may join
// the masthead and the voice guesses are only counted on the details button.
// The details lie in a panel opened over the top of the conversation, where
// the guesses are reviewed; the composer asks and notes either one.
// Esc closes the details, else goes back to the sessions like the close button
// (a live one keeps recording); E edits, R resumes a stored one.
ColumnLayout {
  id: view

  property bool active: false
  property bool compact: false
  // A short window keeps its room for the conversation.
  property bool short: false
  // Whether the capsule lies in the masthead row instead of here, and the
  // language picker in the details.
  property bool joined: false
  property bool detailsOpen: false
  signal detailsRequested()
  signal detailsClosed()

  readonly property bool live: Eco.session !== null
  property bool resuming: false
  readonly property bool resumable: !live && Eco.stored !== null
    && (Eco.stored.state === "interrupted" || Eco.stored.state === "ended")

  spacing: short ? 8 : 14

  function focusEntry() { composer.focusEntry() }

  function resume() {
    if (!resumable || resuming)
      return
    resuming = true
    Eco.reopenSession(Eco.stored.id)
  }

  Connections {
    target: Eco
    function onSessionChanged() {
      if (Eco.session !== null)
        view.resuming = false
    }
    function onNoticeCountChanged() {
      if (view.resuming && Eco.messageIsError)
        view.resuming = false
    }
  }

  focus: active
  onActiveChanged: if (active) {
    if (live)
      focusEntry()
    else
      forceActiveFocus()
  }
  Keys.onEscapePressed: {
    if (detailsOpen)
      detailsClosed()
    else
      Eco.leaveSession()
  }
  Keys.onPressed: event => {
    if (event.key === Qt.Key_R && view.resumable)
      view.resume()
    else if (event.key === Qt.Key_E && Eco.onScreen !== null)
      Eco.renameRequested(Eco.onScreen)
  }

  SessionControl {
    Layout.fillWidth: true
    visible: view.live && !view.joined
    compact: view.compact
  }

  // A stored session: when it was, how long, how much was said; resuming it
  // turns this line into the capsule.
  Rectangle {
    Layout.fillWidth: true
    visible: !view.live && Eco.stored !== null
    implicitHeight: 40
    color: "transparent"
    border.color: Theme.line

    RowLayout {
      anchors { fill: parent; leftMargin: 14; rightMargin: 6 }
      spacing: 10
      Label {
        Layout.fillWidth: true
        text: Eco.stored ? [
          Eco.kindName(Eco.stored.kind),
          I18n.when(Eco.stored.started_at),
          I18n.elapsed(Eco.stored.duration_s),
          I18n.t("history.speech", { n: Eco.stored.speech }),
          Eco.stored.source === "import" ? I18n.t("session.imported") : "",
          Eco.stored.state === "interrupted" ? I18n.t("session.interrupted")
            : Eco.stored.state === "importing" ? I18n.t("import.progress") : ""
        ].filter(part => part).join("  ·  ").toUpperCase() : ""
        color: Theme.dim
        font.pixelSize: 10
        font.letterSpacing: 2
        elide: Text.ElideRight
      }
      TraceButton {
        dense: true
        role: "progress"
        visible: view.resumable
        icon: "play"
        text: view.compact ? "" : Eco.stored && Eco.stored.state === "interrupted" ? I18n.t("session.resume") : I18n.t("detail.reopen")
        Layout.preferredWidth: view.compact ? 32 : -1
        busy: view.resuming
        tip: I18n.t("detail.resume_tip")
        onClicked: view.resume()
      }
    }
  }

  GuessStrip {
    Layout.fillWidth: true
    visible: !view.short && count > 0
    sessionId: Eco.onScreen ? Eco.onScreen.id : ""
    onReviewed: view.detailsRequested()
  }

  Item {
    Layout.fillWidth: true
    Layout.fillHeight: true

    TimelineView {
      anchors.fill: parent
      sessionId: Eco.onScreen ? Eco.onScreen.id : ""
      partials: view.live ? Eco.partials : []
      emptyText: !view.live ? I18n.t("detail.empty")
        : Eco.recording ? I18n.t("timeline.listening") : I18n.t("timeline.paused")
    }

    SessionDetails {
      anchors { left: parent.left; right: parent.right; top: parent.top }
      maxHeight: parent.height
      session: Eco.onScreen
      shown: view.detailsOpen
      compact: view.compact
      languageShown: view.joined && view.compact
      visible: opacity > 0
      opacity: view.detailsOpen ? 1 : 0
      Behavior on opacity { NumberAnimation { duration: 140 } }
    }
  }

  Composer {
    id: composer
    Layout.fillWidth: true
    Layout.fillHeight: false
    sessionId: Eco.onScreen ? Eco.onScreen.id : ""
    compact: view.compact
    actionsShown: !view.short
  }

  StatusLine {
    Layout.alignment: Qt.AlignRight
    message: Eco.message
    error: Eco.messageIsError
    maxWidth: Math.max(0, view.width)
  }
}
