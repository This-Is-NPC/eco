import QtQuick

Column {
  id: samples
  property string speakerColor: ""
  property string chosen: "first"

  spacing: 14

  Kicker { width: parent.width; text: I18n.t("lab.choice_row") }
  Column {
    width: Math.min(320, parent.width)
    Repeater {
      model: [["first", I18n.t("lab.option_first"), "", false], ["second", I18n.t("lab.option_second"), "copy", false], ["third", I18n.t("lab.option_third"), "close", true]]
      delegate: ChoiceRow {
        required property var modelData
        width: parent.width
        text: modelData[1]
        icon: modelData[2]
        guarded: modelData[3]
        chosen: modelData[2] === "" && samples.chosen === modelData[0]
        onPicked: samples.chosen = modelData[0]
      }
    }
  }

  Kicker { width: parent.width; text: I18n.t("lab.suggestion_card") }

  // One answer through its whole life: waiting, reasoning, the trace reaching
  // the end, the reply decoding in word by word, and complete.
  Item {
    id: live
    property string text: ""
    property string thinking: ""
    property bool streaming: false
    property real at: 0
    property int step: 0
    readonly property var words: I18n.t("lab.sample_answer").split(" ")
    width: samples.width
    height: liveCard.height + 36

    function start() {
      text = ""
      thinking = ""
      step = 0
      at = Date.now() / 1000
      streaming = true
      clock.restart()
    }

    Timer {
      id: clock
      interval: 150
      repeat: true
      onTriggered: {
        live.step++
        if (live.step > 8 && live.step <= 20)
          live.thinking += I18n.t("lab.sample_partial").split(" ")[(live.step - 9) % 6] + " "
        else if (live.step > 20 && live.step - 21 < live.words.length)
          live.text += (live.text ? " " : "") + live.words[live.step - 21]
        else if (live.step > 20) {
          live.streaming = false
          stop()
        }
      }
    }

    Chip { id: simulate; text: I18n.t("lab.simulate"); onClicked: live.start() }
    SuggestionCard {
      id: liveCard
      anchors { left: parent.left; right: parent.right; top: simulate.bottom; topMargin: 10 }
      suggestionId: "live"
      text: live.text
      thinking: live.thinking
      failure: ""
      streaming: live.streaming
      action: "probe"
      modelName: "prism-ml/bonsai-27b"
      prompt: ""
      at: live.at
      stamp: "10:04"
    }
  }
  Repeater {
    // Every state of an answer: waiting for the model, reasoning, streaming,
    // complete with its timing and ready to send to its hook, sent, translated, reviewed with
    // its draft shown, a free question, and failed.
    model: [
      { state: I18n.t("lab.card_state.waiting"), text: "", thinking: "", failure: "", streaming: true, action: "probe", prompt: "" },
      { state: I18n.t("lab.card_state.thinking"), text: "", thinking: I18n.t("lab.sample_partial"), failure: "", streaming: true, action: "probe", prompt: "" },
      { state: I18n.t("lab.card_state.streaming"), text: I18n.t("lab.sample_answer"), thinking: "", failure: "", streaming: true, action: "probe", prompt: "" },
      { state: I18n.t("lab.card_state.done"), text: I18n.t("lab.sample_answer"), thinking: "", failure: "", streaming: false, action: "probe", prompt: "", hookable: true, timing: JSON.stringify({ ttft: 350, total: 3400, cache: 93 }) },
      { state: I18n.t("lab.card_state.sent"), text: I18n.t("lab.sample_answer"), thinking: "", failure: "", streaming: false, action: "probe", prompt: "", hookable: true, hookState: "sent" },
      { state: I18n.t("lab.card_state.translated"), text: I18n.t("lab.sample_answer"), thinking: "", failure: "", streaming: false, action: "probe", prompt: "", translation: I18n.t("lab.sample_answer_translated") },
      { state: I18n.t("lab.card_state.reviewed"), text: I18n.t("lab.sample_answer"), thinking: "", failure: "", streaming: false, action: "probe", prompt: "", draft: I18n.t("lab.sample_draft") },
      { state: I18n.t("lab.card_state.question"), text: I18n.t("lab.sample_answer"), thinking: "", failure: "", streaming: false, action: "chat", prompt: I18n.t("lab.sample_question") },
      { state: I18n.t("lab.card_state.failed"), text: "", thinking: "", failure: "no reply for 120 s", streaming: false, action: "probe", prompt: "" }
    ]
    delegate: Column {
      id: sample
      required property var modelData
      required property int index
      width: samples.width
      spacing: 6
      Caption { text: sample.modelData.state.toUpperCase() }
      SuggestionCard {
        width: parent.width
        suggestionId: "sample" + sample.index
        text: sample.modelData.text
        thinking: sample.modelData.thinking
        failure: sample.modelData.failure
        draft: sample.modelData.draft || ""
        hookable: !!sample.modelData.hookable
        hookState: sample.modelData.hookState || ""
        translation: sample.modelData.translation || ""
        timing: sample.modelData.timing || ""
        streaming: sample.modelData.streaming
        action: sample.modelData.action
        modelName: "prism-ml/bonsai-27b"
        prompt: sample.modelData.prompt
        at: Date.now() / 1000 - 12
        stamp: "10:03"
      }
    }
  }

  Kicker { width: parent.width; text: I18n.t("lab.note_card") }
  NoteCard {
    width: parent.width
    noteId: "sample"
    text: I18n.t("lab.note_body")
    stamp: "10:05"
  }

  Kicker { width: parent.width; text: I18n.t("lab.speech_turn") }
  ColorPicker {
    chosen: samples.speakerColor
    automatic: Eco.speakerColor("", "sample", I18n.t("lab.sample_label"))
    onPicked: color => samples.speakerColor = color
  }
  SpeechTurn {
    width: parent.width
    who: "sample"
    name: I18n.t("lab.sample_label")
    sessionId: ""
    text: I18n.t("lab.sample_body")
    mine: false
    stamp: "10:00"
    at: 0
    previewColor: samples.speakerColor
  }
  SpeechTurn {
    width: parent.width
    who: "sample"
    name: I18n.t("lab.sample_label")
    sessionId: ""
    text: I18n.t("lab.view_first_item_two")
    mine: false
    stamp: "10:01"
    at: 1
    continued: true
    previewColor: samples.speakerColor
  }
  SpeechTurn {
    width: parent.width
    who: "me"
    name: "me"
    sessionId: ""
    text: I18n.t("lab.sample_reply")
    mine: true
    stamp: "10:02"
    at: 2
    translation: I18n.t("lab.sample_reply_translated")
  }
  SpeechTurn {
    width: parent.width
    who: "sample"
    name: I18n.t("lab.sample_label")
    sessionId: ""
    text: I18n.t("lab.sample_partial")
    mine: false
    stamp: ""
    at: 3
    partial: true
    previewColor: samples.speakerColor
  }
}
