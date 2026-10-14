import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TranslationView is the language a session's lines and answers are translated
// into, below each one, on one line: a dropdown that picks it, or turns it off,
// among the languages other than the session's own. Shown while a model
// translates sessions of its kind, or while it still translates, to turn it off.
RowLayout {
  id: view

  // The session: {id, kind, language, translating}.
  property var session: null
  readonly property string own: session ? (session.language || "").split("-")[0] : ""

  visible: session !== null && (Eco.modelsOf(session.kind).translation !== "" || !!session.translating)
  spacing: 10

  Dropdown {
    options: ["", ...Eco.languageCodes.filter(code => code !== view.own)]
    current: view.session ? view.session.translating || "" : ""
    describe: code => code ? Eco.languageName(code) : I18n.t("translate.off")
    label: I18n.t("translate.state", { language: describe(current) })
    onPicked: code => Eco.setTranslation(view.session.id, code)
  }
  Item { Layout.fillWidth: true }
}
