import QtQuick
import QtQuick.Layouts

// ContextView is which context slots a session sends with its requests, on one
// line: a chip per slot, lit while on. Shown only when slots are configured.
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
    onChanged: slots => Eco.setContexts(view.session.id, slots)
  }
}
