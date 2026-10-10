import QtQuick

Column {
  id: samples

  property bool showError: false
  property bool advanced: false
  property int unseen: 2
  property bool linkBack: false

  spacing: 14

  Kicker { width: parent.width; text: I18n.t("lab.status_message") }
  Chip {
    text: I18n.t("lab.toggle_status")
    onClicked: samples.showError = !samples.showError
  }
  Flow {
    width: parent.width
    height: childrenRect.height
    spacing: 10
    Repeater {
      model: ["compact", "toast"]
      delegate: Column {
        id: statusVariant
        required property string modelData
        width: Math.max(230, (samples.width - 10) / 2)
        spacing: 6
        Label {
          text: (statusVariant.modelData === "compact" ? I18n.t("lab.status_variant.compact")
            : I18n.t("lab.status_variant.toast")).toUpperCase()
          color: Theme.dim
          font.pixelSize: 10
          font.letterSpacing: 1.5
        }
        StatusMessage {
          width: parent.width
          variant: statusVariant.modelData
          kind: samples.showError ? "error" : "success"
          message: samples.showError ? I18n.t("lab.status_error") : I18n.t("lab.status_success")
          Chip { visible: statusVariant.modelData === "toast"; text: I18n.t("dialog.cancel") }
        }
      }
    }
  }

  ControlLabRow {
    width: parent.width
    title: I18n.t("lab.progress_indicator")
    ProgressTrack {
      width: Math.min(420, samples.width - 210)
      height: 32
      fraction: samples.advanced ? 0.82 : 0.42
      Label {
        anchors.centerIn: parent
        text: Math.round(parent.fraction * 100) + "%"
        font.pixelSize: 11
      }
      MouseArea {
        anchors.fill: parent
        cursorShape: Qt.PointingHandCursor
        onClicked: samples.advanced = !samples.advanced
      }
    }
  }

  ControlLabRow {
    width: parent.width
    title: I18n.t("lab.transcription_strip")
    Column {
      spacing: 10
      Chip {
        text: I18n.t("lab.toggle_transcription")
        onClicked: samples.linkBack = !samples.linkBack
      }
      TranscriptionStrip {
        width: Math.min(420, samples.width - 210)
        sources: [
          { who: I18n.t("lab.sample_speaker"), color: Theme.inputPalette[0], state: samples.linkBack ? "back" : "down", code: "stt.down", detail: I18n.t("lab.sample_outage"), dropped_s: 0 },
          { who: I18n.t("lab.sample_person"), color: Theme.inputPalette[1], state: samples.linkBack ? "down" : "back", code: "stt.down", detail: I18n.t("lab.sample_outage"), dropped_s: 31 }
        ]
      }
    }
  }

  ControlLabRow {
    width: parent.width
    title: I18n.t("lab.new_pill")
    Row {
      spacing: 14
      Chip {
        text: I18n.t("lab.add_entry")
        onClicked: samples.unseen++
      }
      NewPill {
        count: samples.unseen
        onClicked: samples.unseen = 0
      }
    }
  }

  ControlLabRow {
    width: parent.width
    title: I18n.t("lab.confirmation_dialog")
    Row {
      spacing: 8
      Chip {
        text: I18n.t("lab.open_dialog")
        onClicked: confirmation.open()
      }
      Chip {
        text: I18n.t("lab.open_choice_dialog")
        onClicked: choice.open()
      }
    }
  }

  ControlLabRow {
    width: parent.width
    title: I18n.t("lab.dialog_footer")
    DialogFooter {
      width: Math.min(420, samples.width - 210)
      primaryIcon: "enter"
      primaryText: I18n.t("rename.save")
      Chip { icon: "close"; text: I18n.t("speaker.not_them"); dim: true }
    }
  }

  ConfirmDialog {
    id: confirmation
    title: I18n.t("lab.confirmation_dialog")
    question: I18n.t("lab.dialog_question")
    confirmText: I18n.t("lab.dialog_confirm")
  }

  // A confirmation with something else to choose before it: which way to merge.
  ConfirmDialog {
    id: choice
    property bool swapped: false
    title: I18n.t("people.merge_title")
    question: I18n.t("people.merge_question", swapped
      ? { from: I18n.t("lab.sample_speaker"), into: I18n.t("lab.sample_person") }
      : { from: I18n.t("lab.sample_person"), into: I18n.t("lab.sample_speaker") })
    confirmIcon: "merge"
    confirmText: I18n.t("people.merge")
    Chip { text: I18n.t("people.merge_swap"); onClicked: choice.swapped = !choice.swapped }
  }
}
