pragma ComponentBehavior: Bound
import QtQuick

Column {
  id: samples

  spacing: 14

  Kicker { width: parent.width; text: I18n.t("lab.masthead") }
  Masthead { width: parent.width; section: I18n.t("lab.sample_label") }
  Masthead {
    width: parent.width
    tight: true
    branded: false
    section: I18n.t("lab.sample_session")
    SessionControl { dense: true }
    IconButton { name: "more"; count: 2; tip: I18n.t("guess.count", { n: 2 }) }
  }

  Repeater {
    model: ["panel", "card"]
    delegate: Column {
      id: surfaceGroup
      required property string modelData
      width: samples.width
      spacing: 8
      Kicker { width: parent.width; text: surfaceGroup.modelData === "panel" ? I18n.t("lab.panel") : I18n.t("lab.card") }
      Flow {
        width: parent.width
        height: childrenRect.height
        spacing: 10
        Repeater {
          model: ["outline", "filled"]
          delegate: Column {
            id: variantGroup
            required property string modelData
            width: Math.max(230, (samples.width - 10) / 2)
            spacing: 6
            Label {
              text: (variantGroup.modelData === "outline" ? I18n.t("lab.variant.outline") : I18n.t("lab.variant.filled")).toUpperCase()
              color: Theme.dim
              font.pixelSize: 10
              font.letterSpacing: 1.5
            }
            LabSurfaceSample {
              width: parent.width
              kind: surfaceGroup.modelData
              variant: variantGroup.modelData
            }
          }
        }
      }
    }
  }
}
