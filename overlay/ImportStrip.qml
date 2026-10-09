import QtQuick
import QtQuick.Layouts

// ImportStrip is the file being imported: its session, how much of the recording
// is transcribed — the fill grows as speech arrives — and a way to stop, which
// keeps what was transcribed.
ProgressTrack {
  id: strip

  readonly property var job: Eco.importing

  fraction: job && job.total_s > 0 ? Math.min(1, job.done_s / job.total_s) : 0
  visible: job !== null
  implicitHeight: 46

  RowLayout {
    anchors { fill: parent; leftMargin: 16; rightMargin: 8 }
    spacing: 12
    Icon { name: "import"; size: 13; color: Theme.primary }
    DecodeLabel { value: I18n.t("import.progress"); color: Theme.primary; font.pixelSize: 10; font.bold: true; font.letterSpacing: 3 }
    Label {
      Layout.fillWidth: true
      text: strip.job ? Eco.titleOf(strip.job) : ""
      font.pixelSize: 12
      elide: Text.ElideRight
    }
    Label { text: Math.round(strip.fraction * 100) + "%"; color: Theme.dim; font.pixelSize: 11; font.letterSpacing: 1 }
    Chip { icon: "close"; dim: true; tip: I18n.t("import.cancel_tip"); onClicked: Eco.cancelImport() }
  }
}
