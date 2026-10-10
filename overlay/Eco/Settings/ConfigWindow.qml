pragma ComponentBehavior: Bound
import QtQuick
import QtQuick.Layouts
import Eco.Core
import Eco.Kit

// ConfigWindow edits a draft of the daemon's config and saves it whole;
// the daemon validates, writes config.toml and restarts capture.
Window {
  id: window

  property var draft: null
  property var saved: null
  property int page: 0
  property bool saving: false
  property bool stale: false
  property bool invalidExtra: false
  // Counts discards, which the model cards follow.
  property int resets: 0
  // A reviewer rewrites every answer while this is on.
  readonly property bool reviewing: draft !== null && !!draft.reviewer && draft.reviewer.enabled
  property string issue: ""
  readonly property bool dirty: invalidExtra || (draft !== null && saved !== null && JSON.stringify(draft) !== JSON.stringify(saved))
  // A save was tried since the last reset: missing values show as problems too.
  property bool tried: false
  // The window closes once the save under way succeeds.
  property bool closing: false
  // What keeps the draft from saving (see Draft.problems), the shown ones —
  // what was typed wrong, and the rest once a save was tried — and their
  // messages by field path.
  readonly property var problems: draft === null ? [] : (invalidExtra ? [{ page: 1, path: "models.extra", key: "settings.invalid_extra", typed: true }] : [])
    .concat(Draft.problems(draft))
  readonly property var shown: problems.filter(problem => tried || problem.typed)
  readonly property var errors: {
    const map = {}
    shown.forEach(problem => { if (map[problem.path] === undefined) map[problem.path] = I18n.t(problem.key) })
    return map
  }
  // Asks the page holding the field at `path` to open its card and give it the keyboard.
  signal revealed(string path)
  // Asks the fields that apply a change only once it is left to apply it now.
  signal committing()

  // Fixed, untranslated: packaging/hypr/eco.lua matches the window by this title.
  title: "eco · configuração"
  // Fits the screen it opens on, with a margin.
  width: Math.min(1040, screen ? screen.width - 96 : 1040)
  height: Math.min(760, screen ? screen.height - 96 : 760)
  minimumWidth: 240
  minimumHeight: 320
  color: Theme.background
  // Shown while the settings are open. The window is created once and then kept:
  // destroying a window makes Qt write its pipeline cache to disk, and the whole
  // interface waits for that, often for seconds.
  visible: Eco.configOpen
  // Closed by the compositor: the window is kept, as above, and the settings
  // close, unless they are saving or have unsaved changes; with changes, it asks
  // first.
  onClosing: close => {
    close.accepted = false
    if (!saving && !dirty)
      Eco.configOpen = false
    else if (!saving)
      confirmClose.open()
  }

  function reset() {
    saved = Eco.config ? JSON.parse(JSON.stringify(Eco.config)) : null
    draft = saved ? JSON.parse(JSON.stringify(saved)) : null
    stale = false
    invalidExtra = false
    tried = false
    resets += 1
    issue = ""
  }
  // update applies `change` to a copy of the draft and clears the last issue.
  function update(change) {
    const next = JSON.parse(JSON.stringify(draft))
    change(next)
    draft = next
    issue = ""
  }
  readonly property var translation: Draft.translation(draft || {})
  // setTranslation sets one translation field, or removes it for null.
  function setTranslation(key, value) {
    update(d => {
      d.translation = Draft.translation(d)
      if (value === null)
        delete d.translation[key]
      else
        d.translation[key] = value
    })
  }
  // Saves the draft, unless something keeps it from saving: then every problem
  // shows on its field and the first one's tab and field come into view.
  // Returns whether a save started.
  function save() {
    committing()
    if (!dirty || saving || stale)
      return false
    if (problems.length) {
      tried = true
      page = problems[0].page
      revealed(problems[0].path)
      return false
    }
    const next = JSON.parse(JSON.stringify(draft))
    next.participants.forEach(person => person.name = person.name.trim())
    next.kinds = next.kinds.map(kind => kind.trim())
    next.stt.languages = next.stt.languages.map(code => code.trim().toLowerCase())
    next.actions.forEach(action => action.name = action.name.trim())
    next.models.forEach(model => {
      model.name = model.name.trim()
      model.model = model.model.trim()
    })
    const rename = name => name.trim()
    next.llm.model = rename(next.llm.model)
    next.stt.model = rename(next.stt.model)
    if (next.translation && next.translation.model) next.translation.model = rename(next.translation.model)
    if (next.reviewer) next.reviewer.model = rename(next.reviewer.model)
    next.actions.forEach(action => { if (action.model) action.model = rename(action.model) })
    next.contexts = (next.contexts || []).map(slot => Object.assign(slot, { name: slot.name.trim() }))
    const kinds = next.kinds
    // A model serves only the kinds there are, after any were renamed or removed.
    next.models.forEach(model => {
      for (const field of ["kinds", "translates"]) {
        if (model[field])
          model[field] = model[field].filter(kind => kinds.includes(kind))
        if (model[field] && !model[field].length)
          delete model[field]
      }
    })
    issue = ""
    saving = true
    draft = next
    Eco.saveConfig(next)
    return true
  }

  function attemptClose() {
    committing()
    if (saving)
      return
    if (dirty)
      confirmClose.open()
    else
      Eco.configOpen = false
  }

  Connections {
    target: Eco
    function onConfigCloseRequested() { window.attemptClose() }
    function onConfigPageRequested(page) { window.page = page }
    function onConfigChanged() {
      if (!window.dirty)
        window.reset()
      else if (JSON.stringify(Eco.config) !== JSON.stringify(window.saved))
        window.stale = true
    }
    function onConfigSaveSucceeded() {
      if (!window.saving)
        return
      window.saved = JSON.parse(JSON.stringify(window.draft))
      window.saving = false
      window.tried = false
      window.issue = ""
      window.stale = false
      Eco.requestConfig()
      if (window.closing) {
        window.closing = false
        Eco.configOpen = false
      }
    }
    function onConfigSaveFailed(detail) {
      if (!window.saving)
        return
      window.saving = false
      window.closing = false
      window.issue = detail
    }
  }

  Shortcut { sequence: "Ctrl+S"; onActivated: window.save() }
  Shortcut { sequences: ["?", "F1"]; onActivated: shortcuts.open() }
  Shortcut { sequence: "Ctrl+1"; onActivated: window.page = 0 }
  Shortcut { sequence: "Ctrl+2"; onActivated: window.page = 1 }
  Shortcut { sequence: "Ctrl+3"; onActivated: window.page = 2 }
  Shortcut { sequence: "Ctrl+4"; onActivated: window.page = 3 }
  Shortcut { sequence: "Ctrl+5"; onActivated: window.page = 4 }
  Shortcut { sequence: "Ctrl+6"; onActivated: window.page = 5 }
  Shortcut { sequence: "Ctrl+7"; onActivated: window.page = 6 }
  Shortcut { sequence: "Ctrl+8"; onActivated: window.page = 7 }

  // Esc that nothing inside took (an open list or dialog takes its own) closes
  // the window; the window starts with the keyboard here, so Tab enters it.
  ColumnLayout {
    anchors.fill: parent
    focus: true
    Keys.onEscapePressed: window.attemptClose()
    anchors.margins: 28
    spacing: 22

    Masthead {
      Layout.fillWidth: true
      Layout.fillHeight: false
      section: I18n.t("section.config")
      StatusLine { message: Eco.message; error: Eco.messageIsError; maxWidth: Math.max(0, window.width - 380) }
    }

    Flow {
      Layout.fillWidth: true
      spacing: 20
      Repeater {
        model: [I18n.t("settings.audio"), I18n.t("settings.models"), I18n.t("settings.transcription"), I18n.t("settings.answers"),
          I18n.t("settings.skills"), I18n.t("settings.translation_panel"), I18n.t("settings.sessions"), I18n.t("settings.interface")]
        delegate: Tab {
          required property string modelData
          required property int index
          label: modelData
          digit: String(index + 1).padStart(2, "0")
          current: window.page === index
          alert: window.shown.some(problem => problem.page === index)
          onChosen: window.page = index
        }
      }
    }

    Item {
      Layout.fillWidth: true
      Layout.fillHeight: true
      visible: window.draft !== null

      Stage {
        anchors.fill: parent
        shown: window.page === 0
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: audio }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 1
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: modelsPage }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 2
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: transcriptionPage }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 3
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: answersPage }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 4
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: skillsPage }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 5
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: translationPage }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 6
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: sessionsPage }
      }
      Stage {
        anchors.fill: parent
        shown: window.page === 7
        Loader { anchors.fill: parent; active: window.draft !== null; sourceComponent: interfacePage }
      }
    }

    Label {
      visible: window.draft === null
      text: Eco.connected ? I18n.t("config.loading") : I18n.t("start.offline")
      color: Theme.dim
    }

    Label {
      Layout.fillWidth: true
      visible: window.dirty || window.stale || window.issue !== "" || window.shown.length > 0
      text: window.issue || (window.shown.length ? I18n.t("settings.fix_fields", { n: window.shown.length })
        : window.stale ? I18n.t("settings.stale") : I18n.t("settings.unsaved"))
      color: window.issue || window.stale || window.shown.length ? Theme.warning : Theme.dim
      wrapMode: Text.Wrap
      font.pixelSize: 11
    }

    RowLayout {
      Layout.fillWidth: true
      spacing: 8
      Chip { text: I18n.t("settings.close"); enabled: !window.saving; onClicked: window.attemptClose() }
      // Right-aligned from the save button, wrapping on a narrow window rather
      // than widening the whole page. Its preferred width is the buttons' own:
      // the Flow's implicit width changes as it wraps and would never settle.
      Flow {
        Layout.fillWidth: true
        Layout.preferredWidth: saveButton.implicitWidth + discardChip.implicitWidth + spacing
        layoutDirection: Qt.RightToLeft
        spacing: 8
        TraceButton {
          id: saveButton
          dense: true
          role: "progress"
          icon: "enter"
          text: window.saving ? I18n.t("settings.saving") : I18n.t("config.save")
          tip: I18n.t("config.save_tip")
          busy: window.saving
          enabled: window.dirty && !window.stale
          onClicked: window.save()
        }
        Chip { id: discardChip; text: I18n.t("config.discard"); tip: I18n.t("config.discard_tip"); enabled: window.dirty && !window.saving; onClicked: confirmDiscard.open() }
      }
    }
  }

  Component {
    id: audio
    ScrollPage {
      id: scroll
      contentHeight: content.implicitHeight
      AudioPage {
        id: content
        width: scroll.width
        height: Math.max(scroll.height, implicitHeight)
        participants: window.draft.participants
        devices: Eco.devices
        colors: window.draft.colors || {}
        audio: window.draft.audio || {}
        errors: window.errors
        onChanged: participants => window.update(d => d.participants = participants)
        onRecolored: colors => window.update(d => d.colors = colors)
        onAudioEdited: audio => window.update(d => d.audio = audio)
      }
      Connections {
        target: window
        function onRevealed(path) { content.reveal(path) }
      }
    }
  }

  Component {
    id: transcriptionPage
    ScrollPage {
      id: scroll
      contentHeight: content.implicitHeight
      ColumnLayout {
        id: content
        width: scroll.width
        spacing: 16
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "01"
          title: I18n.t("settings.model_panel")
          ColumnLayout {
            anchors.fill: parent
            spacing: 10
            PickRow {
              id: transcriber
              Layout.fillWidth: true
              text: I18n.t("settings.default_pick")
              options: Draft.named(window.draft, "transcription")
              current: window.draft.stt.model
              invalid: window.errors["stt.model"] !== undefined
              onPicked: name => window.update(d => d.stt.model = name)
            }
            FieldError { Layout.fillWidth: true; text: window.errors["stt.model"] || "" }
            Label {
              Layout.fillWidth: true
              text: I18n.t("settings.default_transcription_tip")
              color: Theme.dim
              font.pixelSize: 11
              wrapMode: Text.Wrap
            }
          }
        }
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "02"
          title: I18n.t("config.languages")
          ColumnLayout {
            anchors.fill: parent
            spacing: 10
            SettingsList {
              id: languages
              Layout.fillWidth: true
              items: window.draft.stt.languages
              error: window.errors["stt.languages"] || ""
              choices: ["auto", ...Eco.languageCodes]
              describe: code => Eco.languageName(code)
              onChanged: items => window.update(d => {
                d.stt.languages = items.map(code => code.toLowerCase())
                if (!d.stt.languages.includes(d.stt.language) && d.stt.languages.length)
                  d.stt.language = d.stt.languages[0]
              })
            }
            PickRow {
              text: I18n.t("config.current_language")
              options: window.draft.stt.languages
              current: window.draft.stt.language
              describe: code => Eco.languageName(code)
              onPicked: code => window.update(d => d.stt.language = code)
            }
          }
        }
      }
      Connections {
        target: window
        function onRevealed(path) {
          if (path === "stt.model")
            transcriber.forceActiveFocus()
          else if (path === "stt.languages")
            languages.forceActiveFocus()
        }
      }
    }
  }

  Component {
    id: answersPage
    ScrollPage {
      id: scroll
      contentHeight: content.implicitHeight
      ColumnLayout {
        id: content
        width: scroll.width
        spacing: 16
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "01"
          title: I18n.t("settings.model_panel")
          ColumnLayout {
            anchors.fill: parent
            spacing: 10
            PickRow {
              id: assistant
              Layout.fillWidth: true
              text: I18n.t("settings.default_pick")
              options: Draft.named(window.draft, "chat")
              current: window.draft.llm.model
              invalid: window.errors["llm.model"] !== undefined
              onPicked: name => window.update(d => d.llm.model = name)
            }
            FieldError { Layout.fillWidth: true; text: window.errors["llm.model"] || "" }
            Label {
              Layout.fillWidth: true
              text: I18n.t("settings.default_assistant_tip")
              color: Theme.dim
              font.pixelSize: 11
              wrapMode: Text.Wrap
            }
          }
        }
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "02"
          title: I18n.t("settings.rules")
          SettingsTextArea {
            anchors.fill: parent
            value: window.draft.rules
            onEdited: text => window.update(d => d.rules = text)
          }
        }
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "03"
          title: I18n.t("settings.context")
          ColumnLayout {
            anchors.fill: parent
            spacing: 12
            SettingsList {
              Layout.fillWidth: true
              items: window.draft.context.files
              label: I18n.t("settings.context_files")
              placeholder: I18n.t("settings.file_placeholder")
              browse: true
              nameFilters: Eco.textFileFilters()
              onChanged: items => window.update(d => d.context.files = items)
            }
            ContextSettings {
              id: contexts
              Layout.fillWidth: true
              slots: window.draft.contexts || []
              kinds: window.draft.kinds
              errors: window.errors
              onChanged: slots => window.update(d => d.contexts = slots)
            }
          }
        }
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "04"
          title: I18n.t("settings.reviewer")
          ColumnLayout {
            anchors.fill: parent
            spacing: 12
            Label {
              Layout.fillWidth: true
              text: I18n.t("settings.reviewer_note")
              color: Theme.dim
              font.pixelSize: 11
              wrapMode: Text.Wrap
            }
            Flow {
              Layout.fillWidth: true
              spacing: 6
              Chip {
                text: I18n.t("settings.reviewer_on")
                checked: window.reviewing
                onClicked: {
                  const on = !window.reviewing
                  window.update(d => {
                    const reviewer = d.reviewer || { model: "" }
                    reviewer.enabled = on
                    if (on && !reviewer.model)
                      reviewer.model = d.llm.model
                    d.reviewer = reviewer
                  })
                }
              }
              Chip {
                visible: window.reviewing
                text: I18n.t("settings.reviewer_verbose")
                tip: I18n.t("settings.reviewer_verbose_tip")
                checked: window.reviewing && !!window.draft.reviewer.verbose
                onClicked: window.update(d => d.reviewer.verbose = !d.reviewer.verbose)
              }
            }
            PickRow {
              id: reviewerModel
              visible: window.reviewing
              text: I18n.t("settings.reviewer_model")
              options: Draft.named(window.draft, "chat")
              current: window.reviewing ? window.draft.reviewer.model : ""
              label: current ? "" : I18n.t("settings.pick_model")
              invalid: window.errors["reviewer.model"] !== undefined
              onPicked: name => window.update(d => d.reviewer.model = name)
            }
            FieldError { Layout.fillWidth: true; text: window.errors["reviewer.model"] || "" }
            SettingsTextArea {
              visible: window.reviewing
              Layout.fillWidth: true
              label: I18n.t("settings.reviewer_prompt")
              value: window.reviewing ? (window.draft.reviewer.prompt || "") : ""
              onEdited: text => window.update(d => d.reviewer.prompt = text)
            }
          }
        }
        Panel {
          Layout.fillWidth: true
          Layout.preferredHeight: implicitHeight
          index: "05"
          title: I18n.t("settings.limits")
          ColumnLayout {
            anchors.fill: parent
            spacing: 12
            NumberField {
              Layout.fillWidth: true
              label: I18n.t("config.context")
              number: window.draft.llm.max_context_chars
              onNumberEdited: number => window.update(d => d.llm.max_context_chars = number)
            }
            NumberField {
              Layout.fillWidth: true
              label: I18n.t("config.concurrency")
              number: window.draft.llm.concurrency
              onNumberEdited: number => window.update(d => d.llm.concurrency = number)
            }
          }
        }
      }
      Connections {
        target: window
        function onRevealed(path) {
          if (path === "llm.model")
            assistant.forceActiveFocus()
          else if (path === "reviewer.model")
            reviewerModel.forceActiveFocus()
          else
            contexts.reveal(path)
        }
      }
    }
  }

  Component {
    id: skillsPage
    ScrollPage {
      id: scroll
      contentHeight: pane.implicitHeight
      Panel {
        id: pane
        width: scroll.width
        height: implicitHeight
        index: "01"
        title: I18n.t("settings.actions")
        ActionSettings {
          id: skills
          anchors.fill: parent
          actions: window.draft.actions
          draft: window.draft
          savedNames: window.saved.actions.map(action => action.name)
          errors: window.errors
          onChanged: actions => window.update(d => d.actions = actions)
        }
      }
      Connections {
        target: window
        function onRevealed(path) { skills.reveal(path) }
      }
    }
  }

  Component {
    id: translationPage
    ScrollPage {
      id: scroll
      contentHeight: pane.implicitHeight
      Panel {
        id: pane
        width: scroll.width
        height: implicitHeight
        index: "01"
        title: I18n.t("settings.translation_panel")
        ColumnLayout {
          anchors.fill: parent
          spacing: 12
          Label {
            Layout.fillWidth: true
            text: I18n.t("settings.translation_note")
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
          PickRow {
            Layout.fillWidth: true
            text: I18n.t("settings.default_pick")
            options: ["", ...Draft.named(window.draft, "chat")]
            current: window.translation.model || ""
            describe: name => name || I18n.t("settings.default_model", { name: Draft.answering(window.draft, "chat", "", null).name })
            dim: !Draft.answering(window.draft, "translation", "", null).own
            onPicked: name => window.setTranslation("model", name || null)
          }
          Label {
            Layout.fillWidth: true
            text: I18n.t("settings.default_translation_tip")
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
        }
      }
    }
  }

  Component {
    id: sessionsPage
    ScrollPage {
      id: scroll
      contentHeight: pane.implicitHeight
      Panel {
        id: pane
        width: scroll.width
        height: implicitHeight
        index: "01"
        title: I18n.t("settings.sessions")
        ColumnLayout {
          anchors.fill: parent
          spacing: 12
          Label {
            Layout.fillWidth: true
            text: I18n.t("settings.kind_help")
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
          KindSettings {
            id: kinds
            Layout.fillWidth: true
            draft: window.draft
            errors: window.errors
            onChanged: change => window.update(change)
          }
        }
      }
      Connections {
        target: window
        function onRevealed(path) { kinds.reveal(path) }
        function onCommitting() { kinds.commit() }
      }
    }
  }

  Component {
    id: modelsPage
    ScrollPage {
      id: scroll
      contentHeight: pane.implicitHeight
      Panel {
        id: pane
        width: scroll.width
        height: implicitHeight
        index: "01"
        title: I18n.t("settings.models")
        ColumnLayout {
          anchors.fill: parent
          spacing: 12
          Label {
            Layout.fillWidth: true
            text: I18n.t("settings.models_note")
            color: Theme.dim
            font.pixelSize: 11
            wrapMode: Text.Wrap
          }
          ModelSettings {
            id: models
            Layout.fillWidth: true
            draft: window.draft
            presets: Eco.presets
            resets: window.resets
            errors: window.errors
            onChanged: change => window.update(change)
            onInvalidExtra: invalid => window.invalidExtra = invalid
          }
        }
      }
      Connections {
        target: window
        function onRevealed(path) { models.reveal(path) }
      }
    }
  }

  Component {
    id: interfacePage
    Panel {
      index: "01"
      title: I18n.t("settings.interface")

      ColumnLayout {
        anchors.fill: parent
        spacing: 14

        PickRow {
          text: I18n.t("interface.language")
          options: ["auto", ...I18n.available.map(pack => pack.code)]
          current: window.draft.ui.language
          describe: code => code === "auto" ? I18n.t("interface.auto") : (I18n.packs[code] ? I18n.packs[code]._name : code)
          onPicked: code => window.update(d => d.ui.language = code)
        }

        RowLayout {
          Layout.fillWidth: true
          spacing: 10
          Chip {
            readonly property bool on: window.draft.ui.hide_from_share === true
            text: I18n.t("interface.hide_from_share")
            checked: on
            dim: !on
            onClicked: window.update(d => d.ui.hide_from_share = !on)
          }
          Label {
            Layout.fillWidth: true
            text: I18n.t("interface.hide_from_share_tip")
            color: Theme.dim
            font.pixelSize: 10
            wrapMode: Text.Wrap
          }
        }

        Item { Layout.fillHeight: true }
      }
    }
  }

  ShortcutsDialog { id: shortcuts }
  ConfirmDialog {
    id: confirmDiscard
    title: I18n.t("settings.discard_title")
    question: I18n.t("settings.discard_question")
    confirmText: I18n.t("settings.discard_changes")
    cancelText: I18n.t("settings.keep_editing")
    onConfirmed: window.reset()
  }
  // Closing a changed draft saves it first, keeps editing or discards it.
  ConfirmDialog {
    id: confirmClose
    title: I18n.t("settings.close_title")
    question: I18n.t("settings.close_question")
    confirmText: I18n.t("settings.discard_changes")
    cancelText: I18n.t("settings.keep_editing")
    onConfirmed: { window.reset(); Eco.configOpen = false }
    TraceButton {
      dense: true
      role: "progress"
      icon: "enter"
      text: I18n.t("settings.save_close")
      enabled: !window.stale
      onClicked: {
        confirmClose.close()
        window.closing = window.save()
      }
    }
  }
}
