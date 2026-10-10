import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SessionDetails is what a session is and is set up with, opened from the
// masthead over the top of the conversation: its state, start, duration,
// counts and file, who takes part and who each speaker is, its tags, the
// context it sends, the language it is translated into, and opening what it
// cost, copying or deleting it. Taller than `maxHeight`, it scrolls; it
// opens at the speakers while eco has voice guesses for them. On a short
// window it also holds the language picker, which left the header.
Rectangle {
  id: details

  // The session: {id, kind, state, started_at, duration_s, path, people,
  // attendees, tags, contexts, language, translating}.
  property var session: null
  readonly property bool stored: session !== null && session.path !== undefined
  readonly property string sessionId: session ? session.id : ""

  property real maxHeight: Infinity
  property bool shown: false
  property bool compact: false
  property bool languageShown: false

  height: Math.min(rows.implicitHeight + 28, maxHeight)
  // Opened, it asks for the speakers as they are now.
  onShownChanged: if (shown) {
    Eco.requestSpeakers(sessionId)
    page.scrollTo(speakers.guessing ? speakers.y : 0)
  }
  color: Theme.background
  border.color: Theme.line

  // The conversation under the panel never takes its clicks, wheel or hover.
  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    acceptedButtons: Qt.AllButtons
    onWheel: wheel => wheel.accepted = true
  }

  ScrollPage {
    id: page
    anchors { fill: parent; margins: 1 }
    contentHeight: rows.implicitHeight + 26

    ColumnLayout {
      id: rows
      x: 13
      y: 13
      width: parent.width - 26
      spacing: 12

      // What it is: its state, when it started, how long it ran and what it
      // holds; the title is in the masthead, edited from here.
      RowLayout {
        Layout.fillWidth: true
        spacing: 10
        Label {
          Layout.fillWidth: true
          text: details.session ? [
            Eco.stateName(details.session.state),
            I18n.when(details.session.started_at),
            I18n.elapsed(Eco.ranFor(details.sessionId) ?? details.session.duration_s),
            I18n.t("history.speech", { n: Eco.countOf(details.sessionId, "speech") }),
            I18n.t("details.answers", { n: Eco.countOf(details.sessionId, "suggestion") }),
            I18n.t("details.notes", { n: Eco.countOf(details.sessionId, "note") })
          ].join("  ·  ").toUpperCase() : ""
          color: Theme.dim
          font.pixelSize: 10
          font.letterSpacing: 2
          wrapMode: Text.Wrap
        }
        IconButton {
          Layout.alignment: Qt.AlignTop
          name: "edit"
          tip: I18n.t("rename.tip")
          onClicked: Eco.renameRequested(details.session)
        }
      }

      // Where a stored one is kept.
      RowLayout {
        Layout.fillWidth: true
        visible: details.stored
        spacing: 10
        Label {
          Layout.fillWidth: true
          text: details.stored ? details.session.path : ""
          color: Theme.dim
          font.pixelSize: 10
          elide: Text.ElideMiddle
        }
        TraceButton {
          dense: true
          icon: "copy"
          text: I18n.t("detail.copy_path")
          onClicked: Eco.copy(details.session.path)
        }
      }

      AttendeesView {
        Layout.fillWidth: true
        sessionId: details.sessionId
        peopleIds: details.session ? details.session.people || [] : []
        addedIds: details.session ? details.session.attendees || [] : []
      }

      SpeakersView {
        id: speakers
        Layout.fillWidth: true
        sessionId: details.sessionId
        compact: details.compact
      }

      RowLayout {
        Layout.fillWidth: true
        spacing: 10
        Caption { Layout.alignment: Qt.AlignTop; Layout.topMargin: 9; text: I18n.t("tags.short") }
        TagInput {
          Layout.fillWidth: true
          tags: details.session ? details.session.tags || [] : []
          known: Eco.tagNames
          onAdded: tag => Eco.tagSession(details.session.id, tag)
          onRemoved: tag => Eco.untagSession(details.session.id, tag)
        }
      }

      ContextView {
        Layout.fillWidth: true
        session: details.session
      }

      RowLayout {
        Layout.fillWidth: true
        visible: details.languageShown
        spacing: 10
        Caption { text: I18n.t("dialog.language") }
        LanguagePicker {}
        Item { Layout.fillWidth: true }
      }

      TranslationView {
        Layout.fillWidth: true
        session: details.session
      }

      // What can be done with it: its cost to open, its transcript to copy, a stored one to delete.
      Flow {
        Layout.fillWidth: true
        spacing: 6
        TraceButton {
          dense: true
          icon: "chevron-right"
          text: I18n.t("cost.title")
          tip: I18n.t("cost.tip")
          onClicked: Eco.openCost(details.session.id)
        }
        TraceButton {
          dense: true
          icon: "copy"
          text: I18n.t("detail.copy_vtt")
          onClicked: Eco.copyTranscript(details.session.id)
        }
        TraceButton {
          dense: true
          visible: details.stored
          role: "guarded"
          icon: "trash"
          text: I18n.t("delete.action")
          enabled: details.stored && !Eco.isLive(details.session.id)
          onClicked: Eco.deleteRequested(details.session)
        }
      }
    }
  }
}
