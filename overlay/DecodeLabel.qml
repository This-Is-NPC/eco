import QtQuick

// DecodeLabel reveals a label's changed text from left to right, using the
// selected glyph set until each character resolves. Spacing and punctuation are
// never scrambled, so the text keeps its shape. Labels and short state text
// only: text a reader follows as it grows is drawn as it is.
Label {
  id: label

  property string value
  property bool soft: false
  property bool revealOnStage: true
  readonly property string symbolGlyphs: "▓▒░<>/\\|_-=+*#01"
  readonly property string kanaGlyphs: "アイウエオカキクケコサシスセソタチツテトナニヌネノ"
  readonly property string glyphs: soft ? kanaGlyphs : symbolGlyphs
  property int duration: soft ? 420 : 320
  property bool autoPlay: !soft
  property int elapsed: 0
  property bool ready: false

  text: value
  onValueChanged: {
    if (ready && autoPlay)
      replay()
    else
      text = value
  }
  Component.onCompleted: {
    ready = true
    if (autoPlay)
      replay()
  }

  // Decode the whole value again, as when it comes into view. A decode under
  // way takes a new value on its next frame: text arriving faster than frames
  // never lays the label out more than once a frame.
  function replay() {
    if (!value.length) {
      scramble.stop()
      text = ""
      return
    }
    elapsed = 0
    if (scramble.running)
      return
    updateText()
    scramble.start()
  }

  function updateText() {
    const resolved = Math.floor(value.length * Math.min(1, elapsed / Math.max(1, duration)))
    let output = value.slice(0, resolved)
    for (let i = resolved; i < value.length; i++) {
      const character = value[i]
      output += /[\s.,!?·:*_#`>\-\[\]()|]/.test(character) ? character : glyphs[Math.floor(Math.random() * glyphs.length)]
    }
    text = output
  }

  Timer {
    id: scramble
    interval: 32
    repeat: true
    onTriggered: {
      label.elapsed += interval
      label.updateText()
      if (label.elapsed >= label.duration) {
        stop()
        label.text = label.value
      }
    }
  }
}
