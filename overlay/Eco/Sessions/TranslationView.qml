import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// TranslationView is the language a session's lines and answers are translated
// into, below each one, on one line: a dropdown that picks it, or turns it off,
// among the languages other than the session's own. While no model translates
// sessions of its kind it stays, off, saying why, with a way to the models;
// one that still translates can always be turned off.
RowLayout {
  id: view

  // The session: {id, kind, language, translating}.
  property var session: null
  readonly property string own: session ? (session.language || "").split("-")[0] : ""
  readonly property bool translates: session !== null && Eco.modelsOf(session.kind).translation !== ""

  visible: session !== null
  spacing: 10

  Dropdown {
    options: ["", ...Eco.languageCodes.filter(code => code !== view.own)]
    current: view.session ? view.session.translating || "" : ""
    describe: code => code ? Eco.languageName(code) : I18n.t("translate.off")
    label: I18n.t("translate.state", { language: describe(current) })
    reason: view.session === null || view.translates || !!view.session.translating ? ""
      : I18n.t("error.translation.none", { kind: Eco.kindName(view.session.kind) })
    onPicked: code => Eco.setTranslation(view.session.id, code)
  }
  // Settings › Models, where a chat model that translates is registered.
  Chip {
    visible: !view.translates
    icon: "settings"
    text: I18n.t("settings.open_models")
    onClicked: Eco.openConfigAt(1)
  }
  Item { Layout.fillWidth: true }
}
