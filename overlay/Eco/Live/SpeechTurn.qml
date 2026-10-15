import QtQuick
import Eco.Core
import Eco.Kit

// SpeechTurn is one line of the conversation in a box: the user's on the
// right, everyone else's on the left tinted with their colour, every one at
// full brightness. A line that continues the same speaker's turn drops the name
// and time and sits close to the one before. Clicking the name names that
// speaker in this session; clicking the box asks for the line's menu
// (`clicked`), or picks it while another line waits for a merge. A translated
// session shows each line's translation below it, dimmer.
Item {
  id: turn

  required property string who
  required property string name
  required property string sessionId
  required property string text
  required property bool mine
  required property string stamp
  required property real at
  property bool continued: false
  // The keyboard is on this line.
  property bool selected: false
  // Words still being said: dim, without time or menu, not yet a line.
  property bool partial: false
  property string previewColor: ""
  // The line is being corrected in place, or split; it ends only by
  // confirming or cancelling.
  property bool editing: false
  // The edit open is a split: the text stays, the cursor marks where.
  property bool splitting: false
  // The line in the session's translation language, once it arrives.
  property string translation: ""
  // This line waits for the line it merges with.
  property bool merging: false
  // The box the line's menu opens below.
  readonly property Item box: block
  signal clicked()

  // Opens the line to correct, the cursor at its end.
  function edit() {
    open(false)
    editor.cursorPosition = editor.length
  }
  // Opens the line to split, the cursor at the word nearest its middle.
  function split() {
    open(true)
    const space = turn.text.lastIndexOf(" ", Math.floor(turn.text.length / 2))
    editor.cursorPosition = space > 0 ? space + 1 : Math.floor(turn.text.length / 2)
    // A read-only text hides its cursor; this one marks where the line splits.
    editor.cursorVisible = true
  }
  function open(splits) {
    editor.text = turn.text
    turn.splitting = splits
    turn.editing = true
    editor.forceActiveFocus()
  }
  function cancel() {
    turn.editing = false
    turn.splitting = false
  }
  // Saves the correction, or splits at the cursor; the daemon counts
  // characters, the editor UTF-16 units.
  function confirm() {
    if (turn.splitting) {
      Eco.splitLine(turn.sessionId, turn.who, turn.at, Array.from(editor.text.slice(0, editor.cursorPosition)).length)
    } else {
      const text = editor.text.trim()
      if (text !== turn.text)
        Eco.editLine(turn.sessionId, turn.who, turn.at, text)
    }
    turn.cancel()
  }

  // Who eco guesses the speaker is by voice, until the user confirms or clears it.
  readonly property var guess: Eco.guessOf(turn.sessionId, turn.who)
  readonly property color tone: previewColor ? Theme.readable(previewColor, Theme.background, Theme.foreground)
    : turn.guess ? Eco.personColor(turn.guess.person) : Eco.speakerColor(turn.sessionId, turn.who, turn.name)
  // At most this wide: the whole width on a narrow window, about three
  // quarters of it otherwise.
  readonly property real widest: turn.width < 520 ? turn.width : turn.width * 0.78
  // From the edge of the box to its text.
  readonly property real inset: 10

  implicitHeight: body.y + body.height + 2

  Column {
    id: body
    y: turn.continued ? 2 : 14
    // As wide as the line needs; the whole room while it is corrected.
    width: turn.editing ? turn.widest : Math.min(turn.widest, Math.max(heading.implicitWidth, Math.max(line.implicitWidth, translated.implicitWidth) + 2 * turn.inset))
    // A position, not an anchor: a line can change sides when its speaker does.
    x: turn.mine ? turn.width - width : 0
    spacing: 4

    Row {
      id: heading
      visible: !turn.continued
      spacing: 8
      x: turn.mine ? body.width - width : 0
      Rectangle { width: 5; height: 5; anchors.verticalCenter: parent.verticalCenter; color: turn.tone }
      Label {
        id: speaker
        text: turn.mine && turn.name === turn.who ? I18n.t("timeline.you")
          : (turn.guess ? I18n.t("guess.line", { name: turn.guess.name }) : turn.name).toUpperCase()
        color: turn.guess ? Qt.alpha(turn.tone, 0.7) : turn.tone
        font.pixelSize: 10
        font.letterSpacing: 2.5
        font.underline: pick.containsMouse
        MouseArea {
          id: pick
          anchors.fill: parent
          anchors.margins: -4
          enabled: turn.sessionId !== "" && !turn.partial
          hoverEnabled: true
          cursorShape: Qt.PointingHandCursor
          onClicked: Eco.speakerRequested({ session: turn.sessionId, label: turn.who, name: turn.name, at: turn.at, scope: "speaker" })
        }
        Hint { visible: pick.containsMouse || (turn.selected && !turn.continued); text: I18n.t(turn.guess ? "guess.tip" : "speaker.tip") }
      }
      Label { visible: !turn.partial; text: turn.stamp; color: Qt.alpha(turn.tone, 0.7); font.pixelSize: 9; font.letterSpacing: 1 }
    }

    // The user's own lines sit in a faint block on the right; others' in a
    // block tinted with their speaker's colour.
    Rectangle {
      id: block
      width: parent.width
      height: (turn.editing ? editor.implicitHeight : words.implicitHeight) + 14
      color: turn.mine ? Theme.highlight : Qt.tint(Theme.background, Qt.alpha(turn.tone, 0.1))
      // The line waiting for its merge is outlined in the accent.
      border.color: turn.merging ? Theme.primary : "transparent"
      HoverHandler {
        enabled: tap.enabled
        cursorShape: Qt.PointingHandCursor
      }
      TapHandler {
        id: tap
        enabled: turn.sessionId !== "" && !turn.partial && !turn.editing
        onTapped: turn.clicked()
      }
      TextBlock {
        id: editor
        visible: turn.editing
        anchors.verticalCenter: parent.verticalCenter
        x: turn.inset
        width: parent.width - 2 * turn.inset
        horizontalAlignment: turn.mine ? Text.AlignRight : Text.AlignLeft
        // A split keeps the text; the arrows and a click still move the cursor.
        readOnly: turn.splitting
        selectByKeyboard: true
        onAccepted: turn.confirm()
        Keys.onEscapePressed: turn.cancel()
      }
      Column {
        id: words
        visible: !turn.editing
        anchors.verticalCenter: parent.verticalCenter
        x: turn.inset
        width: parent.width - 2 * turn.inset
        spacing: 3
        Label {
          id: line
          width: parent.width
          horizontalAlignment: turn.mine ? Text.AlignRight : Text.AlignLeft
          text: turn.text
          color: turn.partial ? Theme.dim : Theme.foreground
          font.pixelSize: 12
          lineHeight: 1.2
          wrapMode: Text.Wrap
        }
        Label {
          id: translated
          visible: turn.translation !== ""
          width: parent.width
          horizontalAlignment: turn.mine ? Text.AlignRight : Text.AlignLeft
          text: turn.translation
          color: Theme.dim
          font.pixelSize: 11
          font.italic: true
          wrapMode: Text.Wrap
        }
      }
    }

    Row {
      visible: turn.editing
      x: body.width - width
      spacing: 8
      Chip { text: I18n.t("dialog.cancel"); onClicked: turn.cancel() }
      Chip {
        icon: turn.splitting ? "split" : "enter"
        text: I18n.t(turn.splitting ? "line.split" : "line.save")
        checked: true
        onClicked: turn.confirm()
      }
    }
  }
}
