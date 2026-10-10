pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ProviderFields edit one provider (`stt`, or an `llm` model) — an
// OpenAI-compatible endpoint, or for `stt` a streaming one by its wss:// URL — in
// three steps: which provider (a preset, or a custom base URL), which model
// (typed, or picked from the list the provider offers, which LIST opens and
// closes), and where its key comes from (an environment variable or an omapass
// password).
ColumnLayout {
  id: frame

  required property string target
  required property var section
  property var presets: []
  // What is wrong with the base URL and the model id, by key ("base_url", "model").
  property var errors: ({})
  // The custom endpoint was chosen; a base URL no preset has is custom too.
  property bool customChosen: false
  readonly property bool custom: customChosen || !presets.some(preset => preset.values.base_url === section.base_url)

  signal edited(string key, var value)

  // Gives the keyboard to the field of `key`, "base_url" or "model".
  function focusField(key) {
    if (key === "base_url")
      frame.customChosen = true
    ;(key === "base_url" ? baseUrl : modelId).forceActiveFocus()
  }

  // The omapass passwords are listed once a key taken from there is shown.
  onSectionChanged: {
    const fromOmapass = section && section.api_key_omapass !== undefined
    if (fromOmapass && Eco.omapassAccounts.length === 0 && Eco.omapassError === "")
      Eco.requestOmapass()
  }

  spacing: 18

  ColumnLayout {
    Layout.fillWidth: true
    spacing: 6
    Caption { text: I18n.t("provider.provider") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Repeater {
        model: frame.presets
        delegate: Chip {
          required property var modelData
          text: modelData.name
          checked: !frame.custom && frame.section.base_url === modelData.values.base_url
          onClicked: {
            frame.customChosen = false
            // A key taken from omapass stays there; otherwise the preset names its variable.
            const fromOmapass = frame.section.api_key_omapass !== undefined
            for (const key in modelData.values)
              if (!(fromOmapass && key === "api_key_env"))
                frame.edited(key, modelData.values[key])
            Eco.clearModels(frame.target)
          }
        }
      }
      Chip {
        text: I18n.t("provider.custom")
        checked: frame.custom
        onClicked: frame.customChosen = true
      }
    }
    SyncedBox {
      id: baseUrl
      Layout.fillWidth: true
      visible: frame.custom
      dense: true
      invalid: !!frame.errors.base_url
      value: frame.section.base_url || ""
      placeholderText: I18n.t("provider.base_url")
      onTextEdited: {
        frame.edited("base_url", text)
        Eco.clearModels(frame.target)
      }
    }
    FieldError { Layout.fillWidth: true; text: frame.errors.base_url || "" }
  }

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
        invalid: !!frame.errors.model
        value: frame.section.model || ""
        // The model of the chosen preset, or of the first, as an example.
        placeholderText: {
          const preset = frame.presets.find(p => p.values.base_url === frame.section.base_url) || frame.presets[0]
          return preset ? I18n.t("provider.model_placeholder", { model: preset.values.model }) : ""
        }
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
        onClicked: open ? Eco.clearModels(frame.target) : Eco.requestModels(frame.target, frame.section)
      }
    }
    FieldError { Layout.fillWidth: true; text: frame.errors.model || "" }
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

  ColumnLayout {
    id: key
    Layout.fillWidth: true
    spacing: 6
    // "omapass" while the key is an omapass password; a variable otherwise.
    readonly property string source: frame.section.api_key_omapass !== undefined ? "omapass" : "env"
    Caption { text: I18n.t("provider.key_from") }
    Flow {
      Layout.fillWidth: true
      spacing: 6
      Chip {
        text: I18n.t("provider.key_env")
        checked: key.source === "env"
        dim: !checked
        onClicked: frame.edited("api_key_omapass", null)
      }
      Chip {
        text: I18n.t("provider.key_omapass")
        checked: key.source === "omapass"
        dim: !checked
        tip: I18n.t("provider.key_omapass_tip")
        reason: !Eco.omapassInstalled && !checked ? I18n.t("provider.omapass_missing") : ""
        onClicked: {
          frame.edited("api_key_env", null)
          frame.edited("api_key_omapass", frame.section.api_key_omapass || "")
          Eco.requestOmapass()
        }
      }
      IconButton {
        visible: !Eco.omapassInstalled && Eco.omapassPage !== ""
        name: "info"
        tip: I18n.t("provider.omapass_install")
        onClicked: Qt.openUrlExternally(Eco.omapassPage)
      }
    }
    SyncedBox {
      Layout.fillWidth: true
      visible: key.source === "env"
      dense: true
      value: frame.section.api_key_env || ""
      placeholderText: I18n.t("provider.key_placeholder")
      onTextEdited: frame.edited("api_key_env", text)
    }
    Label {
      Layout.fillWidth: true
      visible: key.source === "omapass" && (!Eco.omapassInstalled || Eco.omapassError !== "" || Eco.omapassAccounts.length === 0)
      text: !Eco.omapassInstalled ? I18n.t("provider.omapass_missing")
          : Eco.omapassError !== "" ? Eco.omapassError.split("\n")[0] : I18n.t("provider.omapass_empty")
      color: !Eco.omapassInstalled || Eco.omapassError !== "" ? Theme.error : Theme.dim
      font.pixelSize: 10
      wrapMode: Text.Wrap
    }
    Dropdown {
      visible: key.source === "omapass" && Eco.omapassAccounts.length > 0
      options: Eco.omapassAccounts.map(account => account.account)
      current: frame.section.api_key_omapass || ""
      label: frame.section.api_key_omapass ? "" : I18n.t("provider.pick_password")
      describe: name => {
        const account = Eco.omapassAccounts.find(a => a.account === name)
        return account && account.folder ? account.folder + " · " + name : name
      }
      onPicked: name => frame.edited("api_key_omapass", name)
    }
  }

  Item { Layout.fillHeight: true }
}
