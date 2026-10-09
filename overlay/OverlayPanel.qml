import QtQuick
import QtQuick.Controls.Basic as C
import QtQuick.Layouts
import Quickshell

// OverlayPanel is eco's window. Without a session: start one, or read the
// past ones. During one: its state, the live inputs, the conversation with
// the AI's answers, and a composer to ask. A normal window, so Hyprland
// floats, pins and moves it; packaging/hypr/eco.lua matches its title.
FloatingWindow {
  id: panel

  title: "eco"
  implicitWidth: 720
  implicitHeight: 720
  minimumSize: Qt.size(240, 320)
  color: Theme.background

  // Below this width, controls drop their labels and keep their icons.
  readonly property bool compact: width < 560
  // Below this height, the window keeps its room for the conversation: tighter
  // margins, the actions only through `/` in the composer, and the voice
  // guesses as a count on the details button.
  readonly property bool short: height < 560
  // A short window wide enough for one header line puts the live session's
  // capsule in the masthead; a compact one then drops the name and folds the
  // language picker into the details.
  readonly property bool joined: short && width >= 360 && current === "session" && Eco.session !== null
  // Whether the session's details lie open over the conversation; another
  // session on screen starts with them closed.
  property bool details: false
  Connections {
    target: Eco
    function onShownChanged() { panel.details = false }
  }

  // A session on screen shows itself, or its cost once the user asked for it.
  readonly property string current: Eco.onScreen !== null ? (Eco.costShown ? "cost" : "session") : Eco.view

  // Closing the window closes the overlay; the daemon keeps any session going and
  // opens it again when the app is opened.
  onClosed: Qt.quit()

  Rectangle {
    anchors.fill: parent
    color: Theme.background
    border.color: Theme.line

    // When the keyboard is left on nothing — a dialog closed, a view changed —
    // a session puts it back in its composer.
    readonly property Item focused: Window.activeFocusItem
    // Nothing, or only the window itself, has the keyboard.
    function lost(item) { return !item || !item.parent || item === panel.contentItem }
    onFocusedChanged: if (lost(focused)) Qt.callLater(() => {
      if (Eco.session !== null && panel.current === "session" && lost(Window.activeFocusItem))
        sessionView.focusEntry()
    })

    ColumnLayout {
      anchors.fill: parent
      anchors.margins: panel.short ? 10 : 20
      spacing: panel.short ? 8 : 18

      Masthead {
        Layout.fillWidth: true
        Layout.fillHeight: false
        tight: panel.short
        branded: !(panel.joined && panel.compact)
        // Start says whether the daemon is there: offline or ready.
        section: panel.current === "session"
          ? Eco.titleOf(Eco.onScreen)
          : panel.current === "cost"
            ? I18n.t("section.cost", { title: Eco.titleOf(Eco.onScreen) })
          : panel.current === "start"
            ? I18n.t(Eco.connected ? "section.start" : "section.offline")
            : I18n.t("section." + panel.current)
        StatusLine {
          message: panel.current === "session" || panel.current === "cost" ? "" : Eco.message
          error: Eco.messageIsError
          // Start already says it in its section.
          offline: !Eco.connected && panel.current !== "start"
          maxWidth: Math.max(0, panel.width - 360)
        }
        SessionControl { visible: panel.joined; dense: true }
        LanguagePicker { visible: Eco.session !== null && panel.current === "session" && !(panel.joined && panel.compact); compact: panel.compact || panel.short }
        IconButton {
          readonly property int guesses: panel.short && Eco.onScreen !== null ? Eco.guessesOf(Eco.onScreen.id).length : 0
          visible: panel.current === "session"
          name: "more"
          count: guesses
          tip: I18n.t("details.tip") + (guesses > 0 ? "\n" + I18n.t("guess.count", { n: guesses }) : "")
          highlighted: panel.details
          onClicked: panel.details = !panel.details
        }
        IconButton {
          name: "settings"
          tip: I18n.t("settings.tip")
          highlighted: Eco.configOpen
          onClicked: Eco.toggleConfig()
        }
        // Closing the session on screen goes back to the sessions; a live one
        // keeps recording.
        IconButton {
          visible: panel.current === "session"
          name: "close"
          tip: I18n.t("session.close_tip")
          onClicked: { panel.details = false; Eco.leaveSession() }
        }
      }

      Item {
        Layout.fillWidth: true
        Layout.fillHeight: true

        // Start: one way forward, and the way back to what was recorded; live
        // sessions are reached from the sessions list.
        Stage {
          id: home
          anchors.fill: parent
          shown: panel.current === "start"
          focus: shown
          onShownChanged: if (shown) forceActiveFocus()
          Keys.onPressed: event => {
            if (event.key === Qt.Key_N)
              beginButton.press()
            else if (event.key === Qt.Key_H)
              historyChip.press()
            else if (event.key === Qt.Key_I)
              importButton.press()
            else if (event.key === Qt.Key_P)
              peopleChip.press()
          }

          readonly property real buttonHeight: panel.short ? 36 : 44
          readonly property real buttonWidth: Math.min(260, panel.width - 40)

          ImportStrip { id: importStrip; anchors { left: parent.left; right: parent.right; top: parent.top } }

          // Below the import, the buttons, scrolling when the window is too short for them.
          ScrollPage {
            id: field
            anchors { left: parent.left; right: parent.right; bottom: parent.bottom; top: importStrip.visible ? importStrip.bottom : parent.top; topMargin: importStrip.visible ? 8 : 0 }
            contentHeight: Math.max(height, startColumn.height)

            // Whole pixels, or the first button's top hairline is not drawn;
            // never past the bottom while it fits.
            Column {
              id: startColumn
              anchors.horizontalCenter: parent.horizontalCenter
              y: Math.round(Math.max(0, Math.min(field.height - height, (field.height - height) / 2 + Math.min(90, Math.max(0, field.height / 6)))))
              spacing: panel.short ? 8 : 14
              TraceButton {
                id: beginButton
                anchors.horizontalCenter: parent.horizontalCenter
                implicitWidth: home.buttonWidth
                implicitHeight: home.buttonHeight + 4
                role: "primary"
                icon: "plus"
                text: I18n.t("start.begin")
                enabled: Eco.connected
                onClicked: start.open()
              }
              TraceButton {
                id: historyChip
                anchors.horizontalCenter: parent.horizontalCenter
                implicitWidth: home.buttonWidth
                implicitHeight: home.buttonHeight
                role: "navigation"
                icon: "list"
                text: I18n.t("start.history")
                enabled: Eco.connected
                tip: I18n.t("start.history_tip")
                onClicked: Eco.openAllHistory()
              }
              TraceButton {
                id: importButton
                anchors.horizontalCenter: parent.horizontalCenter
                implicitWidth: home.buttonWidth
                implicitHeight: home.buttonHeight
                role: "action"
                icon: "import"
                text: I18n.t("start.import")
                enabled: Eco.connected
                tip: I18n.t("start.import_tip")
                onClicked: Eco.importRequested("")
              }
              TraceButton {
                id: peopleChip
                anchors.horizontalCenter: parent.horizontalCenter
                implicitWidth: home.buttonWidth
                implicitHeight: home.buttonHeight
                role: "navigation"
                icon: "people"
                text: I18n.t("start.people")
                enabled: Eco.connected
                tip: I18n.t("start.people_tip")
                onClicked: Eco.openPeople()
              }
              DecodeLabel {
                anchors.horizontalCenter: parent.horizontalCenter
                width: Math.max(0, panel.width - 40)
                value: Eco.connected ? I18n.t("start.session") : I18n.t("start.offline")
                soft: true
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WordWrap
                color: Eco.connected ? Theme.dim : Theme.warning
                font.pixelSize: 10
                font.letterSpacing: 2
              }
            }
          }

          // The inputs, live, in the room above the buttons: each in its own
          // colour, flat in silence and swinging only when audio arrives. With
          // too little room they run dimmed behind the buttons.
          Item {
            id: traces
            readonly property real room: startColumn.y - field.contentY - 8
            readonly property bool roomy: room >= 72
            anchors { left: parent.left; right: parent.right }
            y: field.y
            height: roomy ? room : field.height
            opacity: roomy ? 1 : 0.15
            z: -1
            Repeater {
              model: Eco.inputs
              delegate: EcgTrace {
                required property var modelData
                required property int index
                // Each input a few pixels apart, so flat lines never hide one another.
                anchors { left: parent.left; right: parent.right; verticalCenter: parent.verticalCenter; verticalCenterOffset: (index - (Eco.inputs.length - 1) / 2) * 12 }
                height: Math.min(240, parent.height)
                reach: 0.9
                source: Eco.signals[modelData.id] || null
                tone: Eco.inputColor(modelData.id)
              }
            }
          }
        }

        Stage {
          anchors.fill: parent
          shown: panel.current === "history"
          HistoryView { anchors.fill: parent; active: parent.shown }
        }

        Stage {
          anchors.fill: parent
          shown: panel.current === "people"
          PeopleView { anchors.fill: parent; active: parent.shown }
        }

        Stage {
          anchors.fill: parent
          shown: panel.current === "cost"
          CostView { anchors.fill: parent; active: parent.shown }
        }

        // The session on screen, live or stored. The keyboard starts in a live
        // one's composer.
        Stage {
          anchors.fill: parent
          shown: panel.current === "session"
          SessionView {
            id: sessionView
            anchors.fill: parent
            active: parent.shown
            compact: panel.compact
            short: panel.short
            joined: panel.joined
            detailsOpen: panel.details
            onDetailsRequested: panel.details = true
            onDetailsClosed: panel.details = false
          }
        }
      }
    }

    StartDialog { id: start }
    RenameDialog { id: rename }
    ConfirmDialog {
      id: deleteDialog
      property var session: null
      title: I18n.t("delete.title")
      question: I18n.t("delete.question", { title: session ? Eco.titleOf(session) : "" })
      warning: I18n.t("delete.warning")
      confirmText: I18n.t("delete.action")
      onConfirmed: Eco.deleteSession(session.id)
    }
    // A change to hooks, context files or models another client asked for:
    // only the user's choice here closes it.
    ConfirmDialog {
      id: pendingDialog
      readonly property var change: Eco.pendingConfig
      closePolicy: C.Popup.NoAutoClose
      title: I18n.t("pending.title")
      question: I18n.t("pending.question")
      warning: Eco.pendingText
      cancelText: I18n.t("pending.reject")
      confirmIcon: "check"
      confirmText: I18n.t("pending.approve")
      onChangeChanged: change === null ? close() : open()
      onConfirmed: Eco.approveConfig()
      onCancelled: Eco.rejectConfig()
    }
    ImportDialog { id: importer }
    ShortcutsDialog { id: shortcuts }
    Shortcut { sequences: ["?", "F1"]; onActivated: shortcuts.open() }
    AssignmentDialog { id: speaker }

    // A file dropped anywhere on the window is offered for import.
    DropArea {
      id: drop
      anchors.fill: parent
      keys: ["text/uri-list"]
      onDropped: event => {
        if (event.hasUrls)
          Eco.importRequested(Eco.localPath(event.urls[0]))
      }
    }
    Rectangle {
      anchors { fill: parent; margins: 10 }
      visible: drop.containsDrag
      color: Qt.alpha(Theme.background, 0.85)
      border.color: Theme.primary
      Column {
        anchors.centerIn: parent
        spacing: 12
        Icon { anchors.horizontalCenter: parent.horizontalCenter; name: "import"; size: 28; color: Theme.primary }
        Label { text: I18n.t("import.drop"); color: Theme.primary; font.pixelSize: 11; font.letterSpacing: 3 }
      }
    }
    Connections {
      target: Eco
      function onNewSessionRequested() { start.open() }
      function onRenameRequested(session) { rename.edit(session) }
      function onDeleteRequested(session) {
        if (Eco.isLive(session.id))
          return Eco.tell([["error.session.live"]], true)
        deleteDialog.session = session
        deleteDialog.open()
      }
      // A session resumed while its deletion is asked is no longer deletable.
      function onLiveChanged() {
        if (deleteDialog.session && Eco.isLive(deleteDialog.session.id))
          deleteDialog.close()
      }
      function onImportRequested(path) { importer.choose(path) }
      function onSpeakerRequested(request) { speaker.edit(request) }
    }
  }
}
