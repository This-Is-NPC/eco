import QtQuick
import QtQuick.Controls.Basic as C
import QtQuick.Layouts

// SettingsTextArea is a tracked label, left out when empty because the panel
// around it already names it, over a multi-line input where Enter breaks the
// line; it grows with its text up to `lines` lines, then scrolls inside.
// `edited` carries the new text, and `value` updates it without a binding, so
// typing never detaches it.
ColumnLayout {
  id: field
  property string label
  property string value
  property int lines: 10
  signal edited(string text)
  spacing: 6

  Caption { visible: field.label !== ""; text: field.label.toUpperCase() }
  Flickable {
    id: frame
    Layout.fillWidth: true
    Layout.preferredHeight: Math.min(input.implicitHeight, Math.ceil(metrics.lineSpacing * field.lines) + input.topPadding + input.bottomPadding)
    clip: true
    boundsBehavior: Flickable.StopAtBounds
    // A text that fits leaves the wheel to the page around it.
    interactive: contentHeight > height
    C.TextArea.flickable: TextBlock {
      id: input
      submits: false
      text: field.value
      onTextChanged: if (activeFocus) field.edited(text)
    }
  }
  FontMetrics { id: metrics; font: input.font }
  Connections {
    target: field
    function onValueChanged() { if (input.text !== field.value) input.text = field.value }
  }
}
