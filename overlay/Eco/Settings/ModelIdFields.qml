pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ModelIdFields edit the id a provider knows a model by: typed, or picked from
// the list the provider offers, which LIST opens and closes. The list is the
// saved provider's, so it asks for the provider by name.
ColumnLayout {
  id: frame

  required property string target
  required property var section
  // The model type's first preset, as the example the id's placeholder shows.
  property var example: null
  // What is wrong with the model id.
  property string error: ""

  signal edited(string key, var value)

  // Gives the keyboard to the id.
  function focusField() { modelId.forceActiveFocus() }

  spacing: 18

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("provider.model").toUpperCase() }
    RowLayout {
      Layout.fillWidth: true
      spacing: 6
      SyncedBox {
        id: modelId
        Layout.fillWidth: true
        dense: true
        invalid: !!frame.error
        value: frame.section.model || ""
        // The model of the chosen preset, or of the first, as an example.
        placeholderTextColor: Theme.placeholder
        placeholderText: frame.example ? I18n.t("provider.model_placeholder", { model: frame.example.model }) : ""
        onTextEdited: frame.edited("model", text)
      }
      TraceButton {
        // Open: the list is shown, or its error or emptiness is; pressing again closes it.
        readonly property bool open: Eco.modelsAsked[frame.target] && !Eco.modelsLoading[frame.target]
        dense: true
        role: "progress"
        icon: open ? "close" : "list"
        text: I18n.t(open ? "provider.list_close" : "provider.list")
        tip: I18n.t(open ? "provider.list_close_tip" : "provider.list_tip")
        busy: Eco.modelsLoading[frame.target]
        enabled: open || frame.section.provider !== ""
        onClicked: open ? Eco.clearModels(frame.target) : Eco.requestModels(frame.target, frame.section.provider)
      }
    }
    FieldError { Layout.fillWidth: true; text: frame.error }
    Label {
      Layout.fillWidth: true
      visible: Eco.modelsError[frame.target].length > 0
      text: I18n.t("provider.no_models", { detail: Eco.modelsError[frame.target].split("\n")[0] })
      color: Theme.error
      font.pixelSize: 10
      elide: Text.ElideRight
    }
    Label {
      Layout.fillWidth: true
      visible: Eco.modelsAsked[frame.target] && !Eco.modelsLoading[frame.target] && Eco.models[frame.target].length === 0 && Eco.modelsError[frame.target].length === 0
      text: I18n.t("settings.models_empty")
      color: Theme.dim
      font.pixelSize: 10
    }
    // The models the provider offers, once listed: filter and pick one, with
    // ↑/↓ and Enter from the filter.
    SurfaceFrame {
      Layout.fillWidth: true
      visible: Eco.models[frame.target].length > 0
      implicitHeight: 170
      ColumnLayout {
        anchors { fill: parent; margins: 6 }
        spacing: 4
        TextBox {
          id: filter
          Layout.fillWidth: true
          dense: true
          placeholderTextColor: Theme.placeholder
          placeholderText: I18n.t("provider.filter", { n: Eco.models[frame.target].length })
          onTextChanged: list.currentIndex = 0
          Keys.onDownPressed: list.incrementCurrentIndex()
          Keys.onUpPressed: list.decrementCurrentIndex()
          onAccepted: if (list.currentItem) list.currentItem.picked()
          Keys.onEscapePressed: Eco.clearModels(frame.target)
        }
        ListView {
          id: list
          Layout.fillWidth: true
          Layout.fillHeight: true
          clip: true
          // A row's outline never lies on the clip's edge, where a fractional
          // scale would cut it.
          topMargin: 1
          bottomMargin: 1
          model: Eco.models[frame.target].filter(id => id.toLowerCase().includes(filter.text.toLowerCase()))
          delegate: ChoiceRow {
            required property string modelData
            required property int index
            width: list.width
            text: modelData
            chosen: modelData === frame.section.model
            current: filter.activeFocus && index === list.currentIndex
            elide: Text.ElideMiddle
            onPicked: frame.edited("model", modelData)
          }
        }
      }
    }
  }

}
