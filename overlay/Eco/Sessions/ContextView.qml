import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ContextView is which context slots a session sends with its requests, on one
// line: a chip per slot, lit while on. Shown only when slots are configured and
// something answers in sessions of its kind.
RowLayout {
  id: view

  // The session: {id, kind, contexts}.
  property var session: null

  visible: Eco.contexts.length > 0 && session !== null && Eco.answersIn(session.kind)
  spacing: 10

  Caption { Layout.alignment: Qt.AlignTop; Layout.topMargin: 7; text: I18n.t("contexts.short") }
  ContextChips {
    Layout.fillWidth: true
    on: Eco.contextsOn(view.session)
    onChanged: slots => Eco.setContexts(view.session.id, slots)
  }
}
