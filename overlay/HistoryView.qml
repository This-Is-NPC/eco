pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts

// HistoryView, the SESSIONS screen, filters stored sessions — live ones, by kind,
// tag, person and what they hold — and opens a selected row: a live one shows in
// this window, any other is read back. `/` goes to the search; ↓ or Enter back to
// the rows, ↑ on the first row back to the search.
ColumnLayout {
  id: history
  spacing: 14

  property bool active: false
  property string kind: ""
  property string tag: ""
  Connections {
    target: Eco
    // A tag renamed stays picked under its new name; one no session carries
    // any more is no longer picked.
    function onTagRenamed(from, to) { if (Eco.sameTag(history.tag, from)) history.tag = to }
    function onTagsChanged() { if (!Eco.tagNames.some(tag => Eco.sameTag(tag, history.tag))) history.tag = "" }
  }
  readonly property var present: {
    const found = Eco.sessions.map(session => session.kind)
    return Eco.kinds.filter(kind => found.includes(kind))
      .concat(found.filter((kind, i) => !Eco.kinds.includes(kind) && found.indexOf(kind) === i))
  }
  readonly property var shown: Eco.sessions.filter(session =>
    (!Eco.historyLive || Eco.isLive(session.id)) &&
    (history.kind === "" || session.kind === history.kind) &&
    (history.tag === "" || (session.tags || []).some(tag => Eco.sameTag(tag, history.tag))) &&
    (Eco.historyPerson === "" || (session.people || []).includes(Eco.historyPerson)) &&
    (Eco.historyFound === null || Eco.historyFound.includes(session.id)))

  readonly property bool filtered: Eco.historyLive || kind !== "" || tag !== "" || Eco.historyPerson !== "" || Eco.historySearch !== ""

  function clearFilters() {
    Eco.historyLive = false
    Eco.historyPerson = ""
    kind = ""
    tag = ""
    search.clear()
    Eco.searchHistory("")
  }

  Keys.onPressed: event => {
    if (event.key === Qt.Key_I)
      importButton.press()
    else if (event.text === "/")
      search.forceActiveFocus()
    else
      return
    event.accepted = true
  }
  Keys.onEscapePressed: backButton.press()

  RowLayout {
    Layout.fillWidth: true
    // The buttons wrap rather than run past a narrow window.
    Flow {
      Layout.fillWidth: true
      Layout.preferredWidth: 0
      spacing: 6
      TraceButton { id: backButton; dense: true; role: "navigation"; icon: "chevron-left"; text: Eco.backText; tip: "esc"; onClicked: Eco.goBack() }
      TraceButton { id: importButton; dense: true; role: "action"; icon: "import"; text: I18n.t("start.import"); tip: I18n.t("start.import_tip"); onClicked: Eco.importRequested("") }
      // Back already returns to People when the user came from there.
      TraceButton { visible: Eco.backTo !== "people"; dense: true; role: "navigation"; icon: "people"; text: I18n.t("start.people"); tip: I18n.t("start.people_tip"); onClicked: Eco.openPeople() }
    }
    Label { Layout.alignment: Qt.AlignTop | Qt.AlignRight; Layout.topMargin: 8; text: I18n.t("history.count", { n: history.shown.length }); color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 3 }
  }

  TextBox {
    id: search
    Layout.fillWidth: true
    dense: true
    placeholderText: I18n.t("history.search")
    Component.onCompleted: text = Eco.historySearch
    // A search cleared or set elsewhere shows here; typing still owns its spaces.
    Connections {
      target: Eco
      function onHistorySearchChanged() {
        if (search.text.split(/\s+/).filter(word => word).join(" ") !== Eco.historySearch)
          search.text = Eco.historySearch
      }
    }
    onTextEdited: typing.restart()
    // The search is asked once typing pauses.
    Timer { id: typing; interval: 250; onTriggered: Eco.searchHistory(search.text) }
    rightPadding: searching.running ? 66 : 10
    // A search typed or asked and not yet answered.
    LoadingTrace {
      id: searching
      anchors { right: parent.right; rightMargin: 10; verticalCenter: parent.verticalCenter }
      width: 48
      height: 12
      running: typing.running || Eco.historySearching
    }
    Keys.onEscapePressed: {
      if (search.text) {
        search.clear()
        Eco.searchHistory("")
      } else {
        table.focusRows()
      }
    }
    Keys.onDownPressed: table.focusRows()
    Keys.onReturnPressed: {
      typing.stop()
      Eco.searchHistory(search.text)
      table.focusRows()
    }
  }

  ImportStrip { Layout.fillWidth: true }
  TranscriptionStrip { Layout.fillWidth: true; sources: Eco.importing !== null ? Eco.outages : [] }

  // One line however many filters, so the rows stay in reach in a small window.
  ChipStrip {
    Layout.fillWidth: true
    visible: history.present.length > 1 || Eco.live.length > 0 || Eco.tags.length > 0 || Eco.historyPerson !== ""
    Chip {
      visible: Eco.historyPerson !== ""
      text: I18n.t("history.person", { name: Eco.personName(Eco.historyPerson, "") })
      checked: true
      onClicked: Eco.historyPerson = ""
    }
    Chip {
      visible: Eco.live.length > 0
      text: I18n.t("live.short") + "  " + Eco.live.length
      icon: "play"
      checked: Eco.historyLive
      dim: !Eco.historyLive
      tip: I18n.t("live.filter_tip")
      onClicked: Eco.historyLive = !Eco.historyLive
    }
    Chip {
      text: I18n.t("history.all")
      checked: history.kind === "" && history.tag === ""
      dim: !checked
      onClicked: {
        history.kind = ""
        history.tag = ""
      }
    }
    Repeater {
      model: history.present.length > 1 ? history.present : []
      delegate: Chip {
        required property string modelData
        text: Eco.kindName(modelData).toUpperCase()
        checked: history.kind === modelData
        dim: history.kind !== modelData
        onClicked: history.kind = modelData
      }
    }
    Rectangle {
      visible: Eco.tags.length > 0
      anchors.verticalCenter: parent.verticalCenter
      width: 1
      height: 14
      color: Theme.line
    }
    Repeater {
      model: Eco.tags
      delegate: TagChip {
        required property var modelData
        tag: modelData.tag
        checked: Eco.sameTag(history.tag, modelData.tag)
        dim: !checked
        onClicked: history.tag = checked ? "" : modelData.tag
        onRenameRequested: Eco.renameRequested({ tag: modelData.tag })
        onDeleteRequested: {
          dropTag.target = modelData
          dropTag.open()
        }
      }
    }
  }

  // The transcribers the live sessions keep running, each with how many it feeds,
  // so no second paid model runs unseen.
  Label {
    Layout.fillWidth: true
    visible: Eco.historyLive && Eco.transcribers.length > 0
    text: I18n.t("live.transcribing") + "   " + Eco.transcribers.map(t => I18n.t(t.sessions > 1 ? "live.transcriber_shared" : "live.transcriber",
      { model: t.model, language: t.language.toUpperCase(), sessions: t.sessions })).join("   ")
    color: Theme.dim
    font.pixelSize: 10
    font.letterSpacing: 1
    elide: Text.ElideRight
  }

  SessionsTable {
    id: table
    Layout.fillWidth: true
    Layout.fillHeight: true
    sessions: history.shown
    active: history.active
    deletable: true
    loading: !Eco.sessionsHeard
    emptyText: !Eco.sessionsHeard ? I18n.t("history.loading")
      : Eco.historySearch ? I18n.t("history.no_match", { text: Eco.historySearch })
      : Eco.historyLive && !Eco.sessions.some(session => table.live(session)) ? I18n.t("history.no_live")
      : history.filtered ? I18n.t("history.no_filter_match")
      : I18n.t("history.empty")
    emptyAction: history.filtered ? I18n.t("history.clear_filters") : ""
    onEmptyActed: {
      history.clearFilters()
      table.focusRows()
    }
    onTopLeft: search.forceActiveFocus()
    onOpened: session => Eco.showSession(session.id)
    onDeleteRequested: session => Eco.deleteRequested(session)
    onBackRequested: backButton.press()
  }

  ConfirmDialog {
    id: dropTag
    // The tag to delete: {tag, sessions}.
    property var target: null
    title: I18n.t("tags.delete_title")
    question: target ? I18n.t("tags.delete_question", { tag: target.tag, n: target.sessions }) : ""
    confirmText: I18n.t("tags.delete")
    onConfirmed: Eco.deleteTag(target.tag)
  }
}
