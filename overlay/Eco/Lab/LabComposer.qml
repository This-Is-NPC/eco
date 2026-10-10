import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit
import Eco.Live

ColumnLayout {
  id: sample

  property string selectedAction: ""
  property string lastSent: ""
  property bool busy: false

  spacing: 10
  Timer { id: finish; interval: 1800; onTriggered: sample.busy = false }

  ComposerSurface {
    Layout.fillWidth: true
    actions: [I18n.t("lab.action.summary"), I18n.t("lab.action.questions"), I18n.t("lab.action.tasks")]
    selectedAction: sample.selectedAction
    placeholder: I18n.t("composer.placeholder")
    busy: sample.busy
    onActionChosen: action => sample.selectedAction = action
    onSubmitted: text => { sample.lastSent = text; sample.busy = true; finish.restart() }
    onNoted: text => sample.lastSent = text
  }

  Label {
    Layout.fillWidth: true
    text: sample.lastSent ? I18n.t("lab.composer_sent", { text: sample.lastSent }) : I18n.t("lab.composer_hint")
    color: Theme.dim
    font.pixelSize: 10
    elide: Text.ElideRight
  }
}
