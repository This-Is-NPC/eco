import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TranslationView is the language a session's lines and answers are translated
// into, below each one, on one line: a dropdown that picks it, or turns it off,
// among the languages other than the session's own.
RowLayout {
  id: view

  // The session: {id, language, translating}.
  property var session: null
  readonly property string own: session ? (session.language || "").split("-")[0] : ""

  visible: session !== null
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
