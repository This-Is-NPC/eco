import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// SessionDetails is a session's SESSION tab: its state, start, duration and
// counts, edited from here, where a stored one is kept, its tags, the context
// it sends, and copying or deleting it.
ScrollPage {
  id: page

  property bool active: false
  // The session: {id, kind, state, started_at, duration_s, path, tags, contexts}.
  property var session: null
  readonly property bool stored: session !== null && session.path !== undefined
  readonly property string sessionId: session ? session.id : ""

  contentHeight: rows.implicitHeight + 24

  ColumnLayout {
    id: rows
    x: 1
    y: 12
    width: page.width - 2
    spacing: 14

    RowLayout {
      Layout.fillWidth: true
      spacing: 10
      Label {
        Layout.fillWidth: true
        text: page.session ? [
          Eco.stateName(page.session.state),
          I18n.when(page.session.started_at),
          I18n.elapsed(Eco.ranFor(page.sessionId) ?? page.session.duration_s),
          I18n.t("history.speech", { n: Eco.countOf(page.sessionId, "lines") }),
          I18n.t("details.answers", { n: Eco.countOf(page.sessionId, "answers") }),
          I18n.t("details.notes", { n: Eco.countOf(page.sessionId, "notes") })
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
        onClicked: Eco.renameRequested(page.session)
      }
    }

    // Where a stored one is kept.
    RowLayout {
      Layout.fillWidth: true
      visible: page.stored
      spacing: 10
      Label {
        Layout.fillWidth: true
        text: page.stored ? page.session.path : ""
        color: Theme.dim
        font.pixelSize: 10
        elide: Text.ElideMiddle
      }
      TraceButton {
        dense: true
        icon: "copy"
        text: I18n.t("detail.copy_path")
        onClicked: Eco.copy(page.session.path)
      }
    }

    RowLayout {
      Layout.fillWidth: true
      spacing: 10
      Caption { Layout.alignment: Qt.AlignTop; Layout.topMargin: 9; text: I18n.t("tags.short") }
      TagInput {
        Layout.fillWidth: true
        tags: page.session ? page.session.tags || [] : []
        known: Eco.tagNames
        onAdded: tag => Eco.tagSession(page.session.id, tag)
        onRemoved: tag => Eco.untagSession(page.session.id, tag)
      }
    }

    ContextView {
      Layout.fillWidth: true
      session: page.session
    }

    Flow {
      Layout.fillWidth: true
      spacing: 6
      TraceButton {
        dense: true
        icon: "copy"
        text: I18n.t("detail.copy_vtt")
        onClicked: Eco.copyTranscript(page.session.id)
      }
      TraceButton {
        dense: true
        visible: page.stored
        role: "guarded"
        icon: "trash"
        text: I18n.t("delete.action")
        enabled: page.stored && !Eco.isLive(page.session.id)
        onClicked: Eco.deleteRequested(page.session)
      }
    }
  }
}
