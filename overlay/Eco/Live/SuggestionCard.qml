import QtQuick
import Eco.Core
import Eco.Kit

// SuggestionCard is an answer inside the timeline: what was asked (an action,
// or the user's question), and the AI's reply. Until anything arrives it draws
// the loading trace, which reaches the end and goes as soon as content comes;
// it says how long the model has been waited for, and its reasoning and then
// its reply grow as they stream, its frame lit, the reply's Markdown drawn as it
// comes, its images as their alt text. Once complete it
// can be copied, and it can be removed — a removed card also leaves the context
// of what comes next. A reply that failed shows why, in red. With the reviewer's
// verbose mode, the draft it rewrote shows dimmed above the reply. When its
// action has a hook, a complete reply can be sent to it, and says how that went.
// A complete reply can be translated; its translation shows below it, dimmer.
// Hovering its heading tells how long it took and its cache share.
SurfaceFrame {
  id: card

  required property string suggestionId
  required property string text
  required property string action
  required property string modelName
  required property string prompt
  required property bool streaming
  required property string thinking
  required property string failure
  // The unreviewed answer a reviewer rewrote; empty unless it is kept to inspect.
  property string draft: ""
  required property real at
  required property string stamp
  // Its action has a hook to send it to; `hookState` is how sending went:
  // "", "sending", "sent" or "failed".
  property bool hookable: false
  property string hookState: ""
  // The reply in the session's translation language, once it arrives.
  property string translation: ""
  // How the finished answer went, as JSON {ttft, total} in ms and {cache} in %
  // (-1 unknown); empty when not measured.
  property string timing: ""
  readonly property var timed: timing ? JSON.parse(timing) : null
  // The timing in the interface language, read on hovering the heading.
  readonly property string timingText: timed === null ? "" : [I18n.t("card.timing", { ttft: timed.ttft, total: (timed.total / 1000).toFixed(1) })]
    .concat(timed.cache >= 0 ? [I18n.t("card.cache", { percent: timed.cache })] : [])
    .join("  ·  ")
  // Its tools are Tab stops; in a list, only while the keyboard is on the card.
  property bool reachable: true

  readonly property bool question: action === "chat"
  // Waiting for the first word of the reply: before anything arrives, or while it reasons.
  readonly property bool waiting: streaming && text.length === 0
  // Nothing has arrived yet, not even reasoning: the loading trace runs only then.
  readonly property bool empty: waiting && thinking.length === 0
  property double now: Date.now()
  readonly property int waited: Math.max(0, Math.round(now / 1000 - at))

  implicitHeight: heading.height + asked.height + asked.anchors.topMargin + drafted.height + drafted.anchors.topMargin + progress.height + progress.anchors.topMargin + body.implicitHeight + (translated.visible ? translated.implicitHeight + 10 : 0) + 46

  border.color: card.failure ? Theme.error : card.streaming ? Theme.primary : hover.hovered ? Theme.dim : Theme.line

  // The wait is counted in seconds while it lasts.
  Timer { running: card.waiting; interval: 1000; repeat: true; triggeredOnStart: true; onTriggered: card.now = Date.now() }

  HoverHandler { id: hover }

  Row {
    id: heading
    anchors { left: parent.left; top: parent.top; leftMargin: 14; topMargin: 12 }
    spacing: 10
    Icon { anchors.verticalCenter: parent.verticalCenter; name: "spark"; size: 11; color: Theme.primary }
    DecodeLabel { value: card.question ? I18n.t("timeline.question") : card.action.toUpperCase(); color: Theme.primary; font.pixelSize: 10; font.letterSpacing: 3 }
    Label { text: card.modelName; color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 1 }
    Label { text: card.stamp; color: Theme.line; font.pixelSize: 9; font.letterSpacing: 1 }
    readonly property string tip: card.timingText
    HoverHandler { id: timingHover }
    Hint { visible: timingHover.hovered && heading.tip.length > 0; text: heading.tip }
  }

  Row {
    anchors { right: parent.right; rightMargin: 14; verticalCenter: heading.verticalCenter }
    spacing: 14

    Label {
      anchors.verticalCenter: parent.verticalCenter
      visible: card.hookState !== ""
      text: ({ sending: I18n.t("card.hook_sending"), sent: I18n.t("card.hook_sent"), failed: I18n.t("card.hook_failed") })[card.hookState] || ""
      color: card.hookState === "failed" ? Theme.error : card.hookState === "sent" ? Theme.primary : Theme.dim
      font.pixelSize: 10
      font.letterSpacing: 2
    }

    IconButton {
      visible: card.hookable && !card.streaming && card.text.length > 0
      enabled: card.hookState !== "sending"
      name: "send"
      size: 13
      tip: I18n.t(card.hookState === "" ? "card.send_tip" : "card.send_again_tip")
      activeFocusOnTab: card.reachable
      onClicked: Eco.sendHook(card.suggestionId)
    }

    IconButton {
      visible: !card.streaming && card.text.length > 0 && !card.failure && card.translation === ""
      name: "translate"
      size: 13
      tip: I18n.t("card.translate_tip")
      activeFocusOnTab: card.reachable
      onClicked: Eco.translateEntry(card.suggestionId)
    }

    IconButton {
      visible: !card.streaming && card.text.length > 0
      name: "copy"
      size: 13
      tip: I18n.t("card.copy_tip")
      activeFocusOnTab: card.reachable
      onClicked: Eco.copy(card.text)
    }

    IconButton {
      name: "close"
      size: 13
      tone: Theme.error
      tip: I18n.t("card.remove_tip")
      activeFocusOnTab: card.reachable
      onClicked: Eco.removeEntry(card.suggestionId)
    }
  }

  // The user's own question, quoted above the answer.
  Label {
    id: asked
    anchors { left: parent.left; right: parent.right; top: heading.bottom; leftMargin: 14; rightMargin: 14; topMargin: card.question ? 10 : 0 }
    height: card.question ? implicitHeight : 0
    visible: card.question
    text: "“" + card.prompt + "”"
    color: Theme.dim
    font.pixelSize: 12
    font.italic: true
    wrapMode: Text.Wrap
  }

  // The loading trace, how long the model has been waited for, and the tail of
  // its reasoning; shown until the trace has reached the end.
  Column {
    id: drafted
    anchors { left: parent.left; right: parent.right; top: asked.bottom; leftMargin: 14; rightMargin: 14; topMargin: visible ? 10 : 0 }
    height: visible ? implicitHeight : 0
    visible: card.draft.length > 0
    spacing: 4
    Label { text: I18n.t("card.draft"); color: Theme.dim; font.pixelSize: 10; font.letterSpacing: 2 }
    Label {
      width: parent.width
      text: Markdown.imageless(card.draft)
      textFormat: Text.MarkdownText
      color: Theme.dim
      font.pixelSize: 12
      wrapMode: Text.Wrap
    }
  }

  Column {
    id: progress
    anchors { left: parent.left; right: parent.right; top: drafted.bottom; leftMargin: 14; rightMargin: 14; topMargin: visible ? 10 : 0 }
    height: visible ? implicitHeight : 0
    visible: card.waiting || trace.visible
    spacing: 6
    Label {
      text: I18n.t(card.thinking ? "card.thinking" : "card.waiting", { seconds: card.waited })
      color: Theme.primary
      font.pixelSize: 10
      font.letterSpacing: 2
    }
    LoadingTrace {
      id: trace
      width: parent.width
      height: 22
      running: card.empty
      backdrop: card.color
    }
    // The reasoning as it arrives; only its last lines show.
    Item {
      width: parent.width
      height: Math.min(reasoning.implicitHeight, reasoning.font.pixelSize * 1.4 * 4)
      visible: card.thinking.length > 0
      clip: true
      Label {
        id: reasoning
        anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
        text: card.thinking
        color: Theme.dim
        font.pixelSize: 11
        font.italic: true
        wrapMode: Text.Wrap
      }
    }
  }

  Label {
    id: body
    anchors { left: parent.left; right: parent.right; top: progress.bottom; leftMargin: 14; rightMargin: 14; topMargin: 10 }
    text: card.failure ? I18n.t("card.failed", { detail: card.failure }) : Markdown.imageless(card.streaming ? Markdown.closed(card.text) : card.text)
    textFormat: card.failure ? Text.PlainText : Text.MarkdownText
    color: card.failure ? Theme.error : Theme.foreground
    font.pixelSize: 13
    lineHeight: 1.2
    wrapMode: Text.Wrap
  }

  Label {
    id: translated
    anchors { left: parent.left; right: parent.right; top: body.bottom; leftMargin: 14; rightMargin: 14; topMargin: 10 }
    visible: card.translation !== ""
    text: Markdown.imageless(card.translation)
    textFormat: Text.MarkdownText
    color: Theme.dim
    font.pixelSize: 12
    font.italic: true
    wrapMode: Text.Wrap
  }
}
