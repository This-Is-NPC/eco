import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// GuessStrip says, on one line, how many speakers of a session eco guessed by
// voice; reviewing them opens the session's details at its speakers.
RowLayout {
  id: strip

  property string sessionId: ""
  readonly property int count: Eco.guessesOf(sessionId).length
  signal reviewed()

  visible: count > 0
  spacing: 10

  Icon { name: "spark"; size: 11; color: Theme.primary }
  Label {
    Layout.fillWidth: true
    text: I18n.t("guess.count", { n: strip.count })
    color: Theme.primary
    font.pixelSize: 10
    font.letterSpacing: 2
    elide: Text.ElideRight
  }
  Chip {
    text: I18n.t("guess.review")
    tip: I18n.t("guess.help")
    onClicked: strip.reviewed()
  }
}
