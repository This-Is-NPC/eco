import QtQuick

// SpeechTurn is one line of the conversation in a box: the user's on the
// right, everyone else's on the left tinted with their colour, every one at
// full brightness. A line that continues the same speaker's turn drops the name
// and time and sits close to the one before. Clicking the name names that speaker in this session; hovering
// the line, or the keyboard on it, shows what can be done to it, in a gutter its
// text never takes. A translated session shows each line's translation below it,
// dimmer.
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
  // Words still being said: dim, without time or tools, not yet a line.
  property bool partial: false
  property string previewColor: ""
  // The line is being corrected in place; it ends only by saving or cancelling.
  property bool editing: false
  // The line in the session's translation language, once it arrives.
  property string translation: ""

  function save() {
    const text = editor.text.trim()
    if (text !== turn.text)
      Eco.editLine(turn.sessionId, turn.who, turn.at, text)
    turn.editing = false
  }

  // Who eco guesses the speaker is by voice, until the user confirms or clears it.
  readonly property var guess: Eco.guessOf(turn.sessionId, turn.who)
  readonly property color tone: previewColor ? Theme.readable(previewColor, Theme.background, Theme.foreground)
    : turn.guess ? Eco.personColor(turn.guess.person) : Eco.speakerColor(turn.sessionId, turn.who, turn.name)
  // Below this width the tools stack in a narrow column, so the text keeps the room.
  readonly property bool narrow: turn.width < 360
  // Room beside the text kept for the line's tools.
  readonly property real gutter: turn.sessionId !== "" ? tools.implicitWidth + (narrow ? 8 : 12) : 0
  // At most this wide: the whole width left beside the gutter on a narrow
  // window, about three quarters of it otherwise.
  readonly property real widest: turn.width < 520 ? turn.width - gutter : Math.min(turn.width * 0.78, turn.width - gutter)
  // From the edge of the box to its text.
  readonly property real inset: 10

  implicitHeight: body.y + body.height + 2

  HoverHandler { id: hover }

  // The line's tools, in the gutter on the side its text leaves free.
  Grid {
    id: tools
    visible: turn.sessionId !== "" && !turn.partial && !turn.editing && (turn.selected || hover.hovered || remove.hovered || pencil.hovered || personButton.hovered)
    columns: turn.narrow ? 1 : 3
    spacing: turn.narrow ? 4 : 12
    y: body.y + block.y + Math.min(0, (block.height - height) / 2)
    x: turn.mine ? body.x - width - (turn.narrow ? 8 : 12) : body.x + body.width + (turn.narrow ? 8 : 12)
    IconButton {
      id: pencil
      name: "edit"
      tip: I18n.t("line.edit_tip")
      activeFocusOnTab: turn.selected
      onClicked: {
        editor.text = turn.text
        turn.editing = true
        editor.forceActiveFocus()
        editor.cursorPosition = editor.length
      }
    }
    IconButton {
      id: personButton
      name: "people"
      tip: I18n.t("assign.line_tip")
      activeFocusOnTab: turn.selected
      onClicked: Eco.speakerRequested({ session: turn.sessionId, label: turn.who, name: turn.name, at: turn.at, scope: "line" })
    }
    IconButton {
      id: remove
      name: "close"
      tone: Theme.error
      tip: I18n.t("line.remove_tip")
      activeFocusOnTab: turn.selected
      onClicked: Eco.removeLine(turn.sessionId, turn.who, turn.at)
    }
  }

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
      TextBlock {
        id: editor
        visible: turn.editing
        anchors.verticalCenter: parent.verticalCenter
        x: turn.inset
        width: parent.width - 2 * turn.inset
        horizontalAlignment: turn.mine ? Text.AlignRight : Text.AlignLeft
        onAccepted: turn.save()
        Keys.onEscapePressed: turn.editing = false
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
      Chip { text: I18n.t("dialog.cancel"); tip: "esc"; onClicked: turn.editing = false }
      Chip { icon: "enter"; text: I18n.t("line.save"); tip: "enter"; checked: true; onClicked: turn.save() }
    }
  }
}
