import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit
import Eco.Live
import Eco.Sessions

// SessionView is a session on screen, live or stored: a live one under its
// capsule, a stored one under a line saying when it was and how long, with the
// way to resume it; then its tabs. CONVERSATION is the timeline, its language
// and translation, and the composer; PEOPLE who takes part and who each speaker
// is; SESSION its details; COST what it cost, asked for only when chosen.
// Esc goes back to the conversation, else back to the sessions like the close
// button (a live one keeps recording); E edits, R resumes a stored one.
ColumnLayout {
  id: screen

  property bool active: false
  property bool compact: false
  // A short window keeps its room for the conversation.
  property bool short: false
  // Whether the capsule lies in the masthead row instead of here.
  property bool joined: false
  // The tab on screen: 0 conversation, 1 people, 2 session, 3 cost.
  property int tab: 0
  readonly property var tabs: [I18n.t("tab.conversation"), I18n.t("attendees.short"), I18n.t("tab.session"), I18n.t("cost.title")]

  readonly property bool live: Eco.session !== null
  property bool resuming: false
  readonly property bool resumable: !live && Eco.stored !== null
    && (Eco.stored.state === "interrupted" || Eco.stored.state === "ended")

  spacing: short ? 8 : 14

  function focusEntry() {
    if (tab === 0 && live)
      composer.focusEntry()
    else
      forceActiveFocus()
  }
  // The height the composer takes at the bottom, with the space above it,
  // and the tab's above that.
  readonly property real composerRoom: tab === 0 ? composer.height + spacing : 0
  readonly property real conversationHeight: pages.height

  // Choosing COST asks what the session cost; leaving it lets that go.
  function choose(index) {
    if (index === tab)
      return
    if (index === 3)
      Eco.openCost(Eco.onScreen.id)
    else if (tab === 3)
      Eco.closeCost()
    tab = index
    focusEntry()
  }

  function resume() {
    if (!resumable || resuming)
      return
    resuming = true
    Eco.reopenSession(Eco.stored.id)
  }

  Connections {
    target: Eco
    // Another session on screen starts at its conversation.
    function onShownChanged() { screen.tab = 0 }
    function onSessionChanged() {
      if (Eco.session !== null)
        screen.resuming = false
    }
    function onNoticed(notice) {
      if (screen.resuming && notice.kind === "error")
        screen.resuming = false
    }
  }

  focus: active
  onActiveChanged: if (active) focusEntry()
  Keys.onEscapePressed: {
    if (tab !== 0)
      choose(0)
    else
      Eco.leaveSession()
  }
  Keys.onPressed: event => {
    if (event.key === Qt.Key_R && screen.resumable)
      screen.resume()
    else if (event.key === Qt.Key_E && Eco.onScreen !== null)
      Eco.renameRequested(Eco.onScreen)
  }

  SessionControl {
    Layout.fillWidth: true
    visible: screen.live && !screen.joined
    compact: screen.compact
  }

  // A stored session: when it was, how long, how much was said; resuming it
  // turns this line into the capsule.
  Rectangle {
    Layout.fillWidth: true
    visible: !screen.live && Eco.stored !== null
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
          I18n.t("history.speech", { n: Eco.countOf(Eco.stored.id, "lines") }),
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
        visible: screen.resumable
        icon: "play"
        text: screen.compact ? "" : Eco.stored && Eco.stored.state === "interrupted" ? I18n.t("session.resume") : I18n.t("detail.reopen")
        Layout.preferredWidth: screen.compact ? 32 : -1
        busy: screen.resuming
        tip: I18n.t("detail.resume_tip")
        onClicked: screen.resume()
      }
    }
  }

  // A source whose transcription is down, or just back, while this session
  // records or its file is imported.
  TranscriptionStrip {
    Layout.fillWidth: true
    sources: screen.live || (Eco.stored !== null && Eco.stored.state === "importing") ? Eco.outages : []
  }

  // The tabs, wrapping on a narrow window.
  Flow {
    Layout.fillWidth: true
    spacing: 18
    Repeater {
      model: screen.tabs
      delegate: Tab {
        required property string modelData
        required property int index
        label: modelData
        digit: String(index + 1).padStart(2, "0")
        current: screen.tab === index
        // PEOPLE says when eco has voice guesses to review.
        alert: index === 1 && guesses.count > 0
        onChosen: screen.choose(index)
      }
    }
  }

  Item {
    id: pages
    Layout.fillWidth: true
    Layout.fillHeight: true

    Stage {
      anchors.fill: parent
      shown: screen.tab === 0

      ColumnLayout {
        anchors.fill: parent
        spacing: screen.spacing

        // What the conversation is heard in and translated into, and the
        // voice guesses waiting to be reviewed.
        RowLayout {
          Layout.fillWidth: true
          spacing: 10
          GuessStrip {
            id: guesses
            Layout.fillWidth: true
            sessionId: Eco.onScreen ? Eco.onScreen.id : ""
            onReviewed: screen.choose(1)
          }
          Item { Layout.fillWidth: true; visible: !guesses.visible }
          LanguagePicker { visible: screen.live; compact: screen.compact || screen.short }
          TranslationView { session: Eco.onScreen }
        }

        TimelineView {
          Layout.fillWidth: true
          Layout.fillHeight: true
          sessionId: Eco.onScreen ? Eco.onScreen.id : ""
          partials: screen.live ? Eco.partials : []
          emptyText: !screen.live ? I18n.t("detail.empty")
            : Eco.recording ? I18n.t("timeline.listening") : I18n.t("timeline.paused")
        }

        Composer {
          id: composer
          Layout.fillWidth: true
          session: Eco.onScreen
          compact: screen.compact
          actionsShown: !screen.short
          maxEntryHeight: screen.short ? 96 : 180
        }
      }
    }

    // Each made when first chosen and let go once it leaves.
    ViewStage {
      anchors.fill: parent
      shown: screen.tab === 1
      view: SessionPeople { session: Eco.onScreen; compact: screen.compact }
    }
    ViewStage {
      anchors.fill: parent
      shown: screen.tab === 2
      view: SessionDetails { session: Eco.onScreen }
    }
    ViewStage {
      objectName: "cost"
      anchors.fill: parent
      shown: screen.tab === 3
      view: CostView { onBackRequested: screen.choose(0) }
    }
  }
}
