import QtQuick
import Eco.Core
import Eco.Live
import Eco.Sessions
import Eco.Settings

// Overlay views driven as the daemon drives them, run by tests/window.rs: each
// check that fails is printed, and the exit status is how many failed.
Item {
  id: root
  width: 420
  height: 600
  property int failed: 0

  function check(name, ok) {
    if (!ok) {
      console.error("FAIL: " + name)
      failed += 1
    }
  }

  // The newest notice's text, or "".
  function told() { return Eco.notices.length > 0 ? Eco.noticeText(Eco.notices[Eco.notices.length - 1]) : "" }
  // Whether an item under `item` shows `text`.
  function shows(item, text) {
    for (const child of item.children) {
      if (child.visible && child.text === text)
        return true
      if (child.visible && shows(child, text))
        return true
    }
    return false
  }
  // The first item under `item` that `test` takes, or null.
  function find(item, test) {
    for (const child of item.children) {
      if (test(child))
        return child
      const found = find(child, test)
      if (found)
        return found
    }
    return null
  }

  SuggestionCard {
    id: card
    width: root.width
    suggestionId: "a1"
    text: ""
    action: "chat"
    modelName: "m"
    prompt: "which queue?"
    streaming: true
    thinking: ""
    failure: ""
    at: Date.now() / 1000
    stamp: "10:00:00"
  }

  // What needs a chat model, in a session of a kind none answers or translates
  // and in settings with none registered: it stays, off, saying why.
  readonly property var chatless: ({ id: "s9", kind: "meeting", language: "pt", translating: "", contexts: null })
  TranslationView { id: translation; width: root.width; session: root.chatless }
  ContextView { id: slots; width: root.width; session: root.chatless }
  Composer { id: composer; width: root.width; session: root.chatless }
  SuggestionCard {
    id: untranslated
    width: root.width
    suggestionId: "a2"
    text: "Kafka"
    action: "chat"
    modelName: "m"
    prompt: "which queue?"
    streaming: false
    thinking: ""
    failure: ""
    translatable: false
    kind: "meeting"
    at: Date.now() / 1000
    stamp: "10:00:00"
  }
  KindModels {
    id: kindModels
    width: root.width
    kind: "meeting"
    draft: ({ kinds: ["meeting"], actions: [], models: [{ name: "w", type: "transcription", kinds: [] }],
      stt: { model: "w" }, llm: { model: "" }, translation: { model: "" } })
  }
  SkillsSection { id: skillsTable; settings: root }

  // An item of the settings with nothing related, and one with a link.
  ItemPage {
    id: alone
    width: root.width
    height: 300
    title: "GENERAL"
    facts: [["language", "English"]]
    related: [{ title: "Models", items: [] }]
  }
  ItemPage {
    id: linked
    width: root.width
    height: 300
    kind: "model"
    title: "m"
    facts: [["type", "chat"]]
    related: [{ title: "Session kinds", items: [{ section: 3, id: "meeting", text: "MEETING" }] }]
  }

  // Restarts the daemon refused while a session records.
  property int refusedRestarts: 0
  Connections {
    target: Eco
    function onRestartRefused() { root.refusedRestarts += 1 }
  }

  Timer {
    interval: 50
    running: true
    onTriggered: {
      Eco.actions = ["probe"]
      Eco.contexts = [{ name: "cv", kinds: ["meeting"] }]
      Eco.kindModels = { meeting: { transcription: "w", chat: "", translation: "", actions: [] } }
      const noChat = I18n.t("error.chat.none", { kind: Eco.kindName("meeting") })
      const noTranslation = I18n.t("error.translation.none", { kind: Eco.kindName("meeting") })
      const picker = root.find(translation, item => item.options !== undefined && item.label !== undefined)
      root.check("the translation picker stays with no model to translate", translation.visible && picker !== null && picker.visible)
      root.check("the translation picker is off and says why", picker !== null && picker.reason === noTranslation)
      root.check("the way to the models shows", root.shows(translation, I18n.t("settings.open_models")))
      const slot = root.find(slots, item => item.text === "cv")
      root.check("the context slots stay with no chat model", slots.visible && slot !== null && slot.visible)
      root.check("a context slot is off and says why", slot !== null && slot.reason === noChat)
      const skill = root.find(composer, item => item.text === "1  PROBE")
      root.check("a skill no model answers stays", skill !== null && skill.visible)
      root.check("a skill no model answers is off and says why", skill !== null && skill.reason === noChat)
      const send = root.find(composer, item => item.icon === "enter" && item.reason !== undefined)
      root.check("SEND stays with no chat model", send !== null && send.visible)
      root.check("SEND is off and says why", send !== null && send.reason === noChat)
      const translate = root.find(untranslated, item => item.name === "translate")
      root.check("translating an answer stays with no model to translate", translate !== null && translate.visible)
      root.check("translating an answer is off and says why", translate !== null && translate.reason === noTranslation)
      const picks = ["settings.type_transcription", "settings.role_assistant", "settings.translation"]
        .map(key => root.find(kindModels, item => item.text === I18n.t(key) && item.reason !== undefined))
      root.check("a kind keeps its assistant and translation picks with no chat model", picks.every(pick => pick !== null && pick.visible))
      root.check("only the picks that need a chat model are off, saying why",
        picks[0].reason === "" && picks[1].reason === I18n.t("settings.needs_chat") && picks[2].reason === I18n.t("settings.needs_chat"))
      root.check("a skill's model reads that there is no chat model",
        skillsTable.rows(Object.assign({}, kindModels.draft, { actions: [{ name: "probe", prompt: "p" }] }))[0].cells[1] === I18n.t("settings.no_chat"))
      // Once a model answers and translates there, they are on again.
      Eco.kindModels = { meeting: { transcription: "w", chat: "m", translation: "m", actions: ["probe"] } }
      root.check("the translation picker is on with a model to translate", picker.reason === "" && !root.shows(translation, I18n.t("settings.open_models")))
      root.check("context slots, skills and SEND are on with a chat model", slot.reason === "" && skill.reason === "" && send.reason === "")
      Eco.kindModels = ({})
      Eco.contexts = []
      Eco.actions = []

      // The wait is told until the first word arrives, not while it streams.
      const waiting = I18n.t("card.waiting", { seconds: card.waited })
      root.check("the card waits before the first word", root.shows(card, waiting))
      card.text = "Kafka"
      root.check("the card no longer waits once words arrive", !root.shows(card, waiting))

      // A capture error goes with the last recording session; others stay.
      const recording = { id: "s1", title: "Retro", kind: "meeting", source: "live", state: "recording", running_since: null }
      Eco.handle({ type: "session", live: [recording] })
      Eco.handle({ type: "error", code: "capture.stopped", params: { detail: "gone" }, message: "capture stopped: gone" })
      root.check("a capture error shows while the session records", Eco.notices.some(notice => notice.kind === "error" && notice.live))
      Eco.handle({ type: "session", live: [] })
      root.check("a capture error goes when the session ends", !Eco.notices.some(notice => notice.live))
      Eco.handle({ type: "session", live: [recording] })
      Eco.handle({ type: "error", code: "transcription.failed", params: { who: "Eles", detail: "x" }, message: "x" })
      Eco.leaveSession()
      root.check("a transcription error goes when the session is left", !Eco.notices.some(notice => notice.live))
      Eco.handle({ type: "error", code: "import.failed", params: { detail: "bad.mp4" }, message: "import: bad.mp4" })
      Eco.handle({ type: "session", live: [] })
      root.check("an import error stays when a session ends", Eco.notices.some(notice => notice.kind === "error" && !notice.live))
      // A newer release is told while checks are on, until it is dismissed.
      const newer = { type: "update", current: "0.4.0", latest: "0.5.0", newer: true, notes: "https://example.invalid/v0.5.0", checked_at: 1, check: true, error: "" }
      Eco.handle(newer)
      root.check("a newer release is told", Eco.updateNotice === "0.5.0")
      Eco.handle(Object.assign({}, newer, { check: false }))
      root.check("no notice while checks are off", Eco.updateNotice === "")
      Eco.handle(Object.assign({}, newer, { newer: false, latest: "0.4.0" }))
      root.check("the same version is no notice", Eco.updateNotice === "")
      Eco.handle(newer)
      Eco.dismissUpdate()
      root.check("a dismissed notice goes", Eco.updateNotice === "")
      // Each stage of an install is said; a failure as an error, with its reason.
      Eco.handle({ type: "update_progress", stage: "installing", version: "0.5.0", detail: "" })
      root.check("an install under way is said", Eco.updating && root.told() === I18n.t("update.stage.installing", { version: "0.5.0" }))
      root.check("the stage is said in words, not as its key", root.told().indexOf("update.stage.") < 0)
      Eco.handle({ type: "update_progress", stage: "failed", version: "0.5.0", detail: "pkexec was dismissed" })
      root.check("a failed install says why", !Eco.updating && Eco.notices[Eco.notices.length - 1].kind === "error" && root.told().indexOf("pkexec was dismissed") >= 0)
      // A restart refused while recording asks in the window, not in a toast.
      const toasts = Eco.notices.length
      Eco.handle({ type: "error", code: "daemon.recording", params: null, message: "a session is recording" })
      root.check("a restart refused while recording asks first", root.refusedRestarts === 1 && Eco.notices.length === toasts)
      // RELATED shows only when something is related.
      root.check("an item with nothing related has no RELATED", !root.shows(alone, I18n.t("settings.related").toUpperCase()))
      root.check("an item with a link has RELATED", root.shows(linked, I18n.t("settings.related").toUpperCase()))
      Qt.exit(root.failed)
    }
  }
}
