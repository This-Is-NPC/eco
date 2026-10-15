import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ContextView is which context slots a session sends with its requests, on one
// line: a chip per slot, lit while on. Shown when slots are configured; while
// nothing answers in sessions of its kind the chips are off and say why.
RowLayout {
  id: view

  // The session: {id, kind, contexts}.
  property var session: null

  visible: Eco.contexts.length > 0 && session !== null
  spacing: 10

  Caption { Layout.alignment: Qt.AlignTop; Layout.topMargin: 7; text: I18n.t("contexts.short") }
  ContextChips {
    Layout.fillWidth: true
    on: Eco.contextsOn(view.session)
    reason: view.session === null || Eco.answersIn(view.session.kind) ? "" : I18n.t("error.chat.none", { kind: Eco.kindName(view.session.kind) })
    onChanged: slots => Eco.setContexts(view.session.id, slots)
  }
}
