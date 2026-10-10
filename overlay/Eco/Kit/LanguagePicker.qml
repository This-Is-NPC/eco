import QtQuick
import Eco.Core

// LanguagePicker switches the open session's transcription language mid-session,
// among the languages listed in the config; each shows its code and native name.
Dropdown {
  // Compact shows only the code on the button; the list keeps the names.
  property bool compact: false
  readonly property string language: Eco.session ? Eco.session.language : Eco.language
  options: Eco.languages
  current: language
  describe: code => Eco.languageName(code)
  label: compact ? language.toUpperCase() : ""
  onPicked: code => { if (Eco.session) Eco.setLanguage(Eco.session.id, code) }
}
