pragma ComponentBehavior: Bound
import QtQuick
import Eco.Core
import Eco.Kit

// GeneralSection is how eco behaves, one setting a row under the title of
// what it is about, its value as its control: the interface language a list,
// numbers a field, the rest switches, each lit when on. Every change saves at
// once; Enter or Space on a row turns its switch, opens its list or starts
// writing its number. The note says what the row picked does. UPDATES also
// tells the version running and the latest release, installs a newer one and
// restarts the daemon, asking first while a session records. UNINSTALL asks
// first too, and whether to delete the data with eco; nothing is deleted
// unless it is turned on.
SettingsSection {
  id: section

  // The rows, by group: [group key, [setting ids]].
  readonly property var groups: [
    ["interface", ["language", "hide_from_share", "tray", "notifications"]],
    ["reviewer", ["review", "show_draft"]],
    ["audio", ["drop_echoes", "echo_cancel"]],
    ["answers", ["max_context_chars", "concurrency"]],
    ["updates", ["update_check", "version", "daemon"]],
    ["uninstall", ["uninstall"]]
  ]
  // UNINSTALL asks what goes with eco: its data stays unless turned on here.
  readonly property ConfirmDialog uninstallDialog: ConfirmDialog {
    id: removal
    property bool everything: false
    title: I18n.t("uninstall.title")
    question: I18n.t(everything ? "uninstall.question_everything" : "uninstall.question_keep")
    warning: everything ? I18n.t("uninstall.warning") : ""
    confirmIcon: "check"
    confirmText: I18n.t("uninstall.confirm").toUpperCase()
    // Each time it opens the data stays, until the chip says otherwise.
    onAboutToShow: everything = false
    onConfirmed: Eco.uninstall(everything)

    Chip {
      text: I18n.t("uninstall.data").toUpperCase()
      checked: removal.everything
      dim: !removal.everything
      onClicked: removal.everything = !removal.everything
    }
  }
  // Why echo cancellation cannot run here, or "".
  readonly property string echoUnavailable: Eco.echoCancelError ? I18n.t("error." + Eco.echoCancelError) : ""
  readonly property var tips: ({
    language: I18n.t("interface.language_tip"),
    hide_from_share: I18n.t("interface.hide_from_share_tip"),
    tray: I18n.t("interface.tray_tip"),
    notifications: I18n.t("interface.notifications_tip"),
    review: I18n.t("general.review_tip"),
    show_draft: I18n.t("general.show_draft_tip"),
    drop_echoes: I18n.t("audio.drop_echoes_tip"),
    echo_cancel: echoUnavailable || I18n.t("audio.echo_cancel_tip"),
    max_context_chars: I18n.t("general.context_tip"),
    concurrency: I18n.t("general.concurrency_tip"),
    update_check: I18n.t("general.update_check_tip"),
    version: Eco.updateState !== null && Eco.updateState.error
      ? I18n.t("update.error_tip", { error: Eco.updateState.error }) : I18n.t("general.version_tip"),
    daemon: I18n.t("general.daemon_tip"),
    uninstall: I18n.t("general.uninstall_tip")
  })
  readonly property var labels: ({
    language: I18n.t("interface.language"),
    hide_from_share: I18n.t("interface.hide_from_share"),
    tray: I18n.t("interface.tray"),
    notifications: I18n.t("interface.notifications"),
    review: I18n.t("general.review"),
    show_draft: I18n.t("general.show_draft"),
    drop_echoes: I18n.t("audio.drop_echoes"),
    echo_cancel: I18n.t("audio.echo_cancel"),
    max_context_chars: I18n.t("general.context"),
    concurrency: I18n.t("general.concurrency"),
    update_check: I18n.t("general.update_check"),
    version: I18n.t("general.version"),
    daemon: I18n.t("general.daemon"),
    uninstall: I18n.t("general.uninstall")
  })
  readonly property var titles: ({
    interface: I18n.t("general.group_interface"),
    reviewer: I18n.t("general.group_reviewer"),
    audio: I18n.t("general.group_audio"),
    answers: I18n.t("general.group_answers"),
    updates: I18n.t("general.group_updates"),
    uninstall: I18n.t("general.group_uninstall")
  })
  // The version row's value: the version running and what is known of the
  // latest, or the stage of the install under way or failed.
  readonly property string versionText: {
    const state = Eco.updateState
    const progress = Eco.updateProgress
    if (progress !== null && progress.stage !== "done")
      return I18n.t("update.stage." + progress.stage, progress)
    if (state === null)
      return I18n.t("update.unknown")
    const key = Eco.updateChecking ? "update.checking" : state.error ? "update.check_failed"
      : state.newer ? "update.newer" : state.latest ? "update.up_to_date" : "update.unchecked"
    const text = I18n.t(key, { current: state.current, latest: state.latest || "" })
    if (state.error || !state.checked_at)
      return text
    const seconds = Math.max(0, Date.now() / 1000 - state.checked_at)
    const checked = seconds < 60 ? I18n.t("update.checked_now")
      : seconds < 3600 ? I18n.t("update.checked_minutes", { minutes: Math.floor(seconds / 60) })
      : I18n.t("update.checked_hours", { hours: Math.floor(seconds / 3600) })
    return text + " · " + checked
  }

  name: I18n.t("settings.general")
  note: tips[settings.picked[settings.at.general] || "language"] || ""
  columns: [{ title: I18n.t("interface.column_setting"), weight: 1 }]
  stacked: [[0]]
  group: "group"
  rows: config => groups.reduce((all, entry) => all.concat(entry[1].map(key => ({ id: key, fixed: true, group: titles[entry[0]],
    cells: [labels[key]], dim: key === "show_draft" && !isOn(config, "review") }))), [])
  toolsTitle: I18n.t("interface.column_value")
  toolsWidth: 280
  // The row's value: the language's list, a number's field, a switch, or a
  // command beside what it acts on. Bound to the saved config, a save
  // refused leaves it as saved.
  tools: Item {
    id: value
    property var entry: ({})
    readonly property string key: value.entry.id || ""
    readonly property bool language: key === "language"
    readonly property bool counts: key === "max_context_chars" || key === "concurrency"
    // The rows whose chip runs a command rather than turning a switch.
    readonly property bool command: key === "version" || key === "daemon" || key === "uninstall"
    readonly property bool on: Eco.config !== null && section.isOn(Eco.config, key)
    function act() {
      if (value.language)
        list.open()
      else if (value.counts)
        field.act()
      else if (value.command)
        run.press()
      else
        toggle.press()
    }
    implicitWidth: section.toolsWidth + 6
    implicitHeight: 30
    Dropdown {
      id: list
      visible: value.language
      anchors.verticalCenter: parent.verticalCenter
      options: ["auto", ...I18n.available.map(pack => pack.code)]
      current: Eco.config !== null ? Eco.config.ui.language : "auto"
      describe: code => section.languageName(code)
      onPicked: code => section.settings.saveNow(d => d.ui.language = code)
    }
    NumberField {
      id: field
      visible: value.counts
      anchors.verticalCenter: parent.verticalCenter
      width: 120
      inline: true
      number: Eco.config !== null && value.counts ? Eco.config.llm[value.key] : 0
      onNumberEdited: count => {
        const key = value.key
        section.settings.saveNow(d => d.llm[key] = count)
      }
    }
    Row {
      visible: !value.language && !value.counts && !value.command
      anchors.verticalCenter: parent.verticalCenter
      spacing: 10
      Chip {
        id: toggle
        text: section.settings.onOff(value.on).toUpperCase()
        checked: value.on
        dim: !value.on
        // Off while it cannot run here, unless it is on: it can always be turned off.
        reason: value.on ? "" : value.key === "echo_cancel" ? section.echoUnavailable
          : value.key === "review" && Eco.config !== null && !Draft.chats(Eco.config) ? I18n.t("settings.needs_chat") : ""
        onClicked: section.toggle(value.key, value.on)
      }
      // The reviewer's model, the one it reviews with when on.
      Label {
        anchors.verticalCenter: parent.verticalCenter
        visible: text !== ""
        width: section.toolsWidth - toggle.width - 10
        text: value.key === "review" && Eco.config !== null ? (Eco.config.reviewer || {}).model || "" : ""
        color: Theme.dim
        font.pixelSize: 10
        elide: Text.ElideRight
      }
    }
    // VERSION: UPDATE to a newer release, else CHECK NOW; DAEMON: RESTART;
    // UNINSTALL: the question first. Each beside what it acts on.
    Row {
      visible: value.command
      anchors.verticalCenter: parent.verticalCenter
      spacing: 10
      Chip {
        id: run
        readonly property bool newer: Eco.updateState !== null && Eco.updateState.newer
        text: (value.key === "uninstall" ? I18n.t("uninstall.button") : value.key === "daemon" ? I18n.t("update.restart")
          : newer ? I18n.t("update.update") : I18n.t("update.check_now")).toUpperCase()
        checked: value.key === "version" && newer
        tip: value.key === "uninstall" ? I18n.t("uninstall.tip") : value.key === "daemon" ? I18n.t("update.restart_tip")
          : newer ? I18n.t("update.update_tip", { version: Eco.updateState.latest }) : I18n.t("update.check_now_tip")
        enabled: Eco.connected && (value.key === "daemon" || !Eco.updating && (value.key === "uninstall" || !Eco.updateChecking))
        onClicked: {
          if (value.key === "uninstall")
            removal.open()
          else if (value.key === "daemon")
            Eco.restartDaemon(false)
          else if (newer)
            Eco.installUpdate(Eco.updateState.latest)
          else
            Eco.checkUpdate()
        }
      }
      Label {
        anchors.verticalCenter: parent.verticalCenter
        width: section.toolsWidth - run.width - 10
        text: value.key === "uninstall" ? ""
          : value.key === "version" ? section.versionText
          : Eco.updateProgress !== null && Eco.updateProgress.stage === "waiting" ? I18n.t("update.stage.waiting", Eco.updateProgress)
          : I18n.t(Eco.connected ? "update.daemon_running" : "update.daemon_offline")
        color: value.key === "version" && Eco.updateProgress !== null && Eco.updateProgress.stage === "failed" ? Theme.error : Theme.dim
        font.pixelSize: 10
        elide: Text.ElideRight
      }
    }
    // A restart the daemon refused while a session records asks before it
    // forces it: restarting stops that recording.
    Loader {
      id: ask
      active: value.key === "daemon"
      sourceComponent: ConfirmDialog {
        title: I18n.t("update.restart_title")
        question: I18n.t("update.restart_question")
        confirmIcon: "check"
        confirmText: I18n.t("update.restart").toUpperCase()
        onConfirmed: Eco.restartDaemon(true)
      }
    }
    Connections {
      target: Eco
      enabled: ask.item !== null
      function onRestartRefused() { ask.item.open() }
    }
  }

  // Whether the switch `key` is on in `config`, with the daemon's defaults.
  function isOn(config, key) {
    const reviewer = config.reviewer || {}
    const audio = Object.assign({ echo_cancel: false, drop_echoes: true }, config.audio)
    return ({
      hide_from_share: config.ui.hide_from_share === true,
      tray: config.ui.tray === true,
      notifications: config.ui.notifications === true,
      review: reviewer.enabled === true,
      show_draft: reviewer.verbose === true,
      drop_echoes: audio.drop_echoes === true,
      echo_cancel: audio.echo_cancel === true,
      update_check: (config.update || {}).check !== false
    })[key] === true
  }
  // Turns the switch `key`, `on` as read, the other way, saved at once;
  // reviewing with no model of its own takes the chat default.
  function toggle(key, on) {
    settings.saveNow(d => {
      if (key === "review")
        Draft.review(d, !on)
      else if (key === "show_draft")
        d.reviewer = Object.assign({ enabled: false, model: "" }, d.reviewer, { verbose: !on })
      else if (key === "drop_echoes" || key === "echo_cancel")
        d.audio = Object.assign({ echo_cancel: false, drop_echoes: true }, d.audio, { [key]: !on })
      else if (key === "update_check")
        d.update = Object.assign({ check: true }, d.update, { check: !on })
      else
        d.ui[key] = !on
    })
  }
  // A language as its own pack names it; "auto" follows the system.
  function languageName(code) {
    return code === "auto" ? I18n.t("interface.auto") : (I18n.packs[code] ? I18n.packs[code]._name : code)
  }
}
