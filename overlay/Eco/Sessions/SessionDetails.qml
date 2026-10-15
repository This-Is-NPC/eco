import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SessionDetails is what a session is and is set up with, opened from the
// masthead over the top of the conversation: its state, start, duration,
// counts and file, who takes part and who each speaker is, its tags, the
// context it sends, the language it is translated into, and copying or
// deleting it; opening what it cost only when the masthead has no room for
// it. Its head (the state, edit and close) stays while the rest, taller than
// `maxHeight`, scrolls; it opens at the speakers while eco has voice guesses
// for them. On a short window it also holds the language picker, which left
// the header.
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
  // Whether it holds the session's COST, which the masthead has no room for.
  property bool costShown: false
  // Asks to close the panel, as Esc does.
  signal closeRequested()

  height: Math.min(head.implicitHeight + 13 + 12 + rows.implicitHeight + 26 + 2, maxHeight)
  // Opened, it asks for the speakers as they are now; laid out first, as it is
  // made when it opens.
  function opened() {
    Eco.requestSpeakers(sessionId)
    rows.forceLayout()
    page.scrollTo(speakers.guessing ? speakers.y : 0)
  }
  onShownChanged: if (shown) opened()
  Component.onCompleted: if (shown) opened()
  color: Theme.background
  border.color: Theme.line

  // The conversation under the panel never takes its clicks, wheel or hover.
  MouseArea {
    anchors.fill: parent
    hoverEnabled: true
    acceptedButtons: Qt.AllButtons
    onWheel: wheel => wheel.accepted = true
  }

  // What it is: its state, when it started, how long it ran and what it
  // holds; the title is in the masthead, edited from here; and the close.
  RowLayout {
    id: head
    anchors { left: parent.left; right: parent.right; top: parent.top; leftMargin: 13; rightMargin: 13; topMargin: 13 }
    spacing: 10
    Label {
      Layout.fillWidth: true
      text: details.session ? [
        Eco.stateName(details.session.state),
        I18n.when(details.session.started_at),
        I18n.elapsed(Eco.ranFor(details.sessionId) ?? details.session.duration_s),
        I18n.t("history.speech", { n: Eco.countOf(details.sessionId, "lines") }),
        I18n.t("details.answers", { n: Eco.countOf(details.sessionId, "answers") }),
        I18n.t("details.notes", { n: Eco.countOf(details.sessionId, "notes") })
      ].join("  ·  ").toUpperCase() : ""
      color: Theme.dim
      font.pixelSize: 10
      font.letterSpacing: 2
      wrapMode: Text.Wrap
      maximumLineCount: 2
      elide: Text.ElideRight
    }
    IconButton {
      Layout.alignment: Qt.AlignTop
      name: "edit"
      tip: I18n.t("rename.tip")
      onClicked: Eco.renameRequested(details.session)
    }
    IconButton {
      Layout.alignment: Qt.AlignTop
      name: "chevron-up"
      tip: I18n.t("details.close_tip")
      onClicked: details.closeRequested()
    }
  }
  // The rule under the head, while what scrolls lies under it.
  Rectangle {
    anchors { left: parent.left; right: parent.right; top: head.bottom; topMargin: 11 }
    height: 1
    visible: page.contentY > 0
    color: Theme.line
  }

  ScrollPage {
    id: page
    anchors { left: parent.left; right: parent.right; top: head.bottom; bottom: parent.bottom; topMargin: 12; leftMargin: 1; rightMargin: 1; bottomMargin: 1 }
    contentHeight: rows.implicitHeight + 26

    ColumnLayout {
      id: rows
      x: 12
      y: 0
      width: parent.width - 24
      spacing: 12

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

      // What can be done with it: its cost to open, when the masthead has no
      // room for it, its transcript to copy, a stored one to delete.
      Flow {
        Layout.fillWidth: true
        spacing: 6
        TraceButton {
          dense: true
          visible: details.costShown
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
