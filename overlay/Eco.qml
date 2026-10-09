pragma Singleton
import QtQuick
import EcoHost

// Eco is the daemon as the overlay sees it: the socket, the session it
// announces, the session and its timeline (speech and suggestions), the live
// signal of each input and the config. It reconnects on its own; every write
// is a one-line command.
Singleton {
  id: root

  readonly property bool connected: link.connected
  property var actions: []
  // The actions with a hook: their answers can be sent to it.
  property var hooks: []
  property var participants: []
  property var inputs: []
  property string language: ""
  // Interface language pack chosen in the config, or "auto".
  property string uiLanguage: "auto"
  property var languages: []
  // The kinds a session can be given, as configured.
  property var kinds: []
  // What sessions of each kind run on, by model name: {kind: {transcription, chat}}.
  property var kindModels: ({})
  // The language codes eco offers to transcribe in and to translate into, as the
  // daemon lists them.
  property var languageCodes: []
  // The context slots the user configured: {name, kinds}.
  property var contexts: []
  // id -> InputSignal, one per input being captured.
  property var signals: ({})
  // This window's number, given by the daemon, and the session it shows — at
  // first the live one the daemon gave it — or "". Each window shows its own.
  readonly property int window: Number(Host.env("ECO_WINDOW") || 0)
  property string shown: Host.env("ECO_SHOW") || ""
  // The session this window shows when it is live ({id, title, kind, source,
  // language, state, started_at, people, attendees, contexts, translating}) or
  // null.
  readonly property var session: live.find(item => item.id === shown) || null
  // The session this window shows when it is stored, as read back: the same
  // fields, with its path, size and how much was said; or null.
  property var stored: null
  // The session on screen, live or stored, or null.
  readonly property var onScreen: session || (stored !== null && stored.id === shown ? stored : null)
  // The session whose timeline the window holds, so another one shown brings its own.
  property string followed: ""
  // Later, not inside the change: following may show another session, and with it
  // change `session` again.
  onSessionChanged: Qt.callLater(follow)
  readonly property bool recording: session !== null && session.state === "recording"
  // The session whose cost screen is open over it, only after the user asked
  // for it, and what it cost as the daemon last said: {llm_usd, llm_unknown,
  // transcription_usd, transcription_unknown, total_usd, transcribed_s, parts,
  // items}, or null until it answers.
  property string costOf: ""
  property var cost: null
  onShownChanged: closeCost()
  readonly property bool costShown: onScreen !== null && costOf === onScreen.id
  function openCost(id) {
    cost = null
    costOf = id
    send("session.cost " + id)
  }
  function closeCost() { costOf = "" }
  // The timeline of the session on screen: speech turns, notes and suggestion
  // cards, oldest first.
  property ListModel timeline: ListModel {}
  // What the window shows without a session on screen: "start", "history" or "people".
  property string view: "start"
  // The views the user came through to `view`; back returns to the last.
  property var trail: []
  readonly property string backTo: trail.length > 0 ? trail[trail.length - 1] : "start"
  // What the way back reads: the screen it returns to.
  readonly property string backText: I18n.t({ start: "history.back", history: "start.history", people: "start.people" }[backTo])
  // go shows `next`, coming back to it if the user came through it.
  function go(next) {
    if (next === view)
      return
    const at = trail.indexOf(next)
    trail = at >= 0 ? trail.slice(0, at) : trail.concat([view])
    view = next
  }
  function goBack() { go(backTo) }
  // Stored sessions, newest first.
  property var sessions: []
  // Whether `sessions` holds what the daemon said: until then the list loads.
  property bool sessionsHeard: false
  // Every tag with how many sessions carry it ({tag, sessions}), by name.
  property var tags: []
  readonly property var tagNames: tags.map(item => item.tag)
  // What each speaker of the open session is saying right now, not yet a line:
  // [{who, name, text}], from a streaming STT.
  property var partials: []
  // Session id -> its speakers as the daemon last said: {label, name, person, voice, suggestions, guess}.
  property var sessionSpeakers: ({})
  // The people linked to sessions ({id, name, voices, sessions}), by name.
  property var people: []
  // Whether `people` holds what the daemon said: until then no one is unknown.
  property bool peopleHeard: false
  // Person id -> colour: the one they chose, or else the first palette colour
  // from where their id points that no one listed wears yet.
  readonly property var personColors: {
    const palette = Theme.inputPalette
    const worn = new Set(people.filter(p => p.color).map(p => p.color))
    const colors = {}
    for (const person of people.slice().sort((a, b) => a.id < b.id ? -1 : 1)) {
      if (person.color) {
        colors[person.id] = identityColor(person.id, person.color)
        continue
      }
      const start = hashOf(person.id) % palette.length
      let at = start
      while (worn.has(palette[at]) && (at + 1) % palette.length !== start)
        at = (at + 1) % palette.length
      const color = worn.has(palette[at]) ? palette[start] : palette[at]
      worn.add(color)
      colors[person.id] = color
    }
    return colors
  }
  property string historyPerson: ""
  // What the history is searched for, and the ids of the sessions that hold it:
  // null while nothing is searched; until an answer comes, the last one stays.
  property string historySearch: ""
  property var historyFound: null
  // The search `historyFound` answers: another one is still being asked.
  property string historyAnswered: ""
  readonly property bool historySearching: historySearch !== "" && historyAnswered !== historySearch
  // Whether the history shows only the sessions recording or paused now, in any
  // window; off once none is.
  property bool historyLive: false
  onLiveChanged: if (live.length === 0) historyLive = false
  // The file being imported ({id, title, kind, done_s, total_s}) or null.
  property var importing: null
  // When the file to import was recorded, as the daemon reads it: {path, at}, `at`
  // null when the file cannot be imported.
  property var importDate: ({ path: "", at: null })
  // The live sessions, recording or paused, in the order they started, from any
  // window; `session` is the one this window shows. Each also carries `active_s`,
  // the seconds it ran before, and `running_since`, when its current run began
  // (null while paused).
  property var live: []
  // The time, in seconds, moved each second while a live session runs.
  property double clock: Date.now() / 1000
  // The transcribers the recording sessions keep running: {model, language, sessions}.
  property var transcribers: []
  // The answers streaming or waiting to, by id: {session, action, at}. Each
  // session answers on its own, so each composer shows only its session's.
  property var answers: ({})
  // The status notice as parts — {key, args} to translate, or {text} as given —
  // rendered in the current language, so it follows a language switch.
  property var notice: []
  readonly property string message: notice.map(part => part.key ? I18n.t(part.key, part.args) : part.text).join("  ·  ")
  property bool messageIsError: false
  // Counts notices, so the status line shows a new one without re-showing an
  // old one whose text merely changed language.
  property int noticeCount: 0

  property bool configOpen: false
  signal configCloseRequested()
  // Opening asks the daemon for the config, so the kept settings window shows
  // what is saved now.
  function toggleConfig() {
    if (configOpen) {
      configCloseRequested()
    } else {
      configOpen = true
      requestConfig()
    }
  }
  // Asks the settings window to show a tab, opening it if closed.
  signal configPageRequested(int page)
  function openConfigAt(page) {
    if (!configOpen)
      toggleConfig()
    configPageRequested(page)
  }
  property var config: null
  // The token the daemon gave this window: its config.set is the user's own.
  readonly property string token: Host.env("ECO_TOKEN") || ""
  // A change another client asked for, held until the user approves or rejects
  // it ({id, hooks: [{action, command}], files, models: [{name, base_url,
  // api_key_env, api_key_omapass}]}), or null. The daemon says it again on
  // connecting.
  property var pendingConfig: null
  // What the held change runs, sends and where, one line each.
  readonly property string pendingText: pendingConfig === null ? "" : pendingConfig.hooks.map(hook => I18n.t("pending.hook", hook))
    .concat(pendingConfig.files.map(file => I18n.t("pending.file", { path: file })))
    .concat(pendingConfig.models.map(model => I18n.t(model.api_key_env ? "pending.model_env" : model.api_key_omapass ? "pending.model_omapass" : "pending.model_keyless", model)))
    .join("\n")
  onConnectedChanged: if (!connected) pendingConfig = null
  // Approve or reject the held change `id`, the one the window shows.
  function approveConfig(id) { send("config.approve " + token + " " + id) }
  function rejectConfig(id) { send("config.reject " + token + " " + id) }
  property var devices: []
  signal configSaveSucceeded()
  signal configSaveFailed(string detail)
  property var models: ({ stt: [], llm: [] })
  property var modelsError: ({ stt: "", llm: "" })
  property var modelsLoading: ({ stt: false, llm: false })
  property var modelsAsked: ({ stt: false, llm: false })
  // The passwords omapass keeps ({account, folder}), and why they could not be listed.
  property var omapassAccounts: []
  property string omapassError: ""
  // Whether omapass is on this machine, and the page that tells how to install it.
  property bool omapassInstalled: true
  property string omapassPage: ""
  // Provider presets by model type, from the daemon: {chat: [...], transcription: [...]},
  // each {name, values} with the model fields it sets.
  property var presets: ({ chat: [], transcription: [] })

  // Asks the overlay to open the new-session dialog, e.g. from a shortcut.
  signal newSessionRequested()
  // Asks the overlay to rename a session ({id, title, kind}), a person ({personId, name})
  // or a tag ({tag}), or to name a person to add ({newPerson: true}).
  signal renameRequested(var target)
  signal deleteRequested(var session)
  // Asks the overlay to name a speaker in a session ({session, label, name}).
  signal speakerRequested(var speaker)
  // Asks the overlay to import a file, its path filled in or "".
  signal importRequested(string path)
  // A tag every session now carries under another name.
  signal tagRenamed(string from, string to)

  // act does what a shortcut asks of this window (a `window_call` event, see
  // docs/design.md §10): "config" opens or closes the settings, "new_session" the
  // new-session dialog, "sessions" lists the sessions when none is on screen,
  // "import" opens the import dialog with `path` filled in.
  function act(call) {
    if (call.call === "config")
      toggleConfig()
    else if (call.call === "new_session")
      newSessionRequested()
    else if (call.call === "sessions" && session === null)
      openAllHistory()
    else if (call.call === "import")
      importRequested(call.path || "")
  }
  // The call the daemon opened this window to make, as JSON, or "": made once
  // the daemon has said what it holds.
  property string opening: Host.env("ECO_CALL")

  function send(command) {
    if (!connected)
      return
    link.send(command)
  }
  // sendHook runs the hook of the action that gave answer `id`, in the open session or the one read back.
  function sendHook(id) {
    const found = locate(id)
    if (found)
      send("hook.send " + JSON.stringify({ session: onScreen.id, id: id }))
  }
  function triggerOn(id, name) { send("session.action " + JSON.stringify({ id: id, name: name })) }
  function askOn(id, question) { send("session.ask " + JSON.stringify({ id: id, question: question })) }
  // setTranslation translates a session into `code` from now on, or stops for "".
  function setTranslation(sessionId, code) { send("session.translation " + JSON.stringify({ id: sessionId, language: code })) }
  // translateEntry translates one answer of the open session or the one read
  // back — into the session's translation language, else the interface's.
  function translateEntry(id) {
    const found = locate(id)
    if (found)
      send("entry.translate " + JSON.stringify({ session: onScreen.id, id: id, language: I18n.code.split("-")[0] }))
  }
  // A note is context later answers use: a fact the user writes, not a question.
  function noteOn(id, text) { send("session.note " + JSON.stringify({ id: id, text: text })) }
  function startSession(title, kind, code, slots, tags) { send("session.start " + JSON.stringify({ title: title, kind: kind, language: code, contexts: slots, tags: tags, window: window })) }
  // The slots sessions of `kind` start with on.
  function contextsOfKind(kind) { return contexts.filter(slot => slot.kinds.includes(kind)).map(slot => slot.name) }
  // The slots a session ({kind, contexts}) has on: the ones chosen, or those of its kind.
  function contextsOn(item) { return item ? (item.contexts || contextsOfKind(item.kind)) : [] }
  function setContexts(sessionId, slots) { send("session.context " + JSON.stringify({ id: sessionId, contexts: slots })) }
  function pauseSession() { if (session) send("session.pause " + session.id) }
  function resumeSession() { if (session) send("session.resume " + session.id) }
  function endSession() { if (session) send("session.end " + session.id) }
  // An answer that failed is already gone from the daemon: it only leaves the screen.
  // Any other answer, or a note, is removed from its session — the open one or the
  // one read back.
  function removeEntry(id) {
    const found = locate(id)
    if (!found)
      return
    if (found.list.get(found.index).failure)
      found.list.remove(found.index)
    else
      send("entry.remove " + JSON.stringify({ session: onScreen.id, id: id }))
  }
  // copy puts text on the Wayland clipboard, as it is, and says what it was.
  function copy(text, notice) {
    Host.copy(text)
    tell([[notice || "status.copied"]], false)
  }
  // The session whose WebVTT the overlay asked for: exports others request are
  // not copied.
  property string exporting: ""
  // The local path a file: URL names, as file dialogs and drops give them.
  function localPath(url) { return decodeURIComponent(url.toString().replace(/^file:\/\//, "")) }
  // The files a context can be read from, for a file dialog.
  function textFileFilters() { return [I18n.t("files.text") + " (*.md *.txt)", I18n.t("import.any") + " (*)"] }
  function copyTranscript(id) { exporting = id; send("session.export " + id) }
  function openHistory() {
    go("history")
    send("sessions")
    if (historySearch)
      send("sessions.search " + historySearch)
  }
  function openAllHistory() { historyPerson = ""; historySearch = ""; historyFound = null; historyLive = false; openHistory() }
  // searchHistory asks which sessions hold `text`, in any case or accents.
  function searchHistory(text) {
    historySearch = text.split(/\s+/).filter(word => word).join(" ")
    if (historySearch)
      send("sessions.search " + historySearch)
    else
      historyFound = null
  }
  function historyForPerson(id) { historyPerson = id; openHistory() }
  // showSession puts a session on screen: a live one as it goes on, a stored one
  // read back.
  function showSession(id) {
    if (live.some(item => item.id === id)) {
      shown = id
      return
    }
    stored = null
    timeline.clear()
    shown = id
    send("session.show " + id)
  }
  function deleteSession(id) { send("session.delete " + id) }
  // isLive tells a session some window records or holds paused now: it cannot be deleted.
  function isLive(id) { return live.some(item => item.id === id) }
  // ranFor gives the seconds the live session `id` has run, pauses left out, or
  // null when it is not live.
  function ranFor(id) {
    const entry = live.find(item => item.id === id)
    if (!entry)
      return null
    return entry.active_s + (entry.running_since === null ? 0 : Math.max(0, clock - entry.running_since))
  }
  // A session resumed here goes on on screen once it is live.
  function reopenSession(id) { shown = id; send("session.reopen " + id) }
  function importFile(request) { send("session.import " + JSON.stringify(request)) }
  function dateImport(path) { send("import.date " + path) }
  function cancelImport() { send("import.cancel") }
  function openPeople() { go("people"); send("people") }
  function addPerson(name) { send("person.add " + JSON.stringify({ name: name })) }
  function assignPerson(session, label, person, name, color) { send("person.assign " + JSON.stringify({ session: session, label: label, person: person, name: name, color: color })) }
  function setPersonColor(person, color) { send("person.color " + JSON.stringify({ person: person, color: color })) }
  function setSpeakerColor(session, label, color) { send("session.speaker_color " + JSON.stringify({ session: session, label: label, color: color })) }
  function assignLine(id, who, at, person, name) { send("person.assign_line " + JSON.stringify({ id: id, who: who, at: at, person: person, name: name })) }
  function unassignPerson(session, label) { send("person.unassign " + JSON.stringify({ session: session, label: label })) }
  function clearGuess(session, label) { send("person.guess.clear " + JSON.stringify({ session: session, label: label })) }
  function addAttendee(session, person, name) { send("person.attend " + JSON.stringify({ session: session, person: person, name: name })) }
  function removeAttendee(session, person) { send("person.leave " + JSON.stringify({ session: session, person: person })) }
  function renamePerson(id, name) { send("person.rename " + JSON.stringify({ id: id, name: name })) }
  function mergePeople(into, from) { send("person.merge " + JSON.stringify({ into: into, from: from })) }
  function forgetPerson(id) { send("person.forget " + id) }
  // speakersOf is what the daemon last said of a session's speakers.
  function speakersOf(sessionId) { return sessionSpeakers[sessionId] || [] }
  function requestSpeakers(sessionId) { send("session.speakers " + sessionId) }
  // guessesOf lists the speakers of a session eco guessed by voice and the user
  // has not confirmed or cleared; guessOf is one speaker's guess, or null.
  function guessesOf(sessionId) { return speakersOf(sessionId).filter(s => !!s.guess) }
  function guessOf(sessionId, label) {
    const speaker = speakersOf(sessionId).find(s => s.label === label)
    return speaker && speaker.guess ? speaker.guess : null
  }
  function keepSpeakers(sessionId, speakers) {
    const kept = Object.assign({}, sessionSpeakers)
    kept[sessionId] = speakers
    sessionSpeakers = kept
  }
  function editLine(id, who, at, text) { send("session.line.edit " + JSON.stringify({ id: id, who: who, at: at, text: text })) }
  function removeLine(id, who, at) { send("session.line.remove " + JSON.stringify({ id: id, who: who, at: at })) }
  function renameSpeaker(id, label, name) { send("session.speaker " + JSON.stringify({ id: id, label: label, name: name })) }
  // countOf counts the entries of a kind ("speech", "suggestion", "note") in a
  // session on screen, only `who`'s lines when given; a binding that calls it
  // follows the timeline's length.
  function countOf(sessionId, kind, who) {
    const list = timelineOf(sessionId)
    let count = 0
    for (let i = 0; list && i < list.count; i++) {
      const entry = list.get(i)
      if (entry.kind === kind && (!who || entry.who === who))
        count++
    }
    return count
  }
  // speakerNames lists the names other speakers go by in a session on screen.
  function speakerNames(sessionId) {
    const list = timelineOf(sessionId)
    const names = []
    for (let i = 0; list && i < list.count; i++) {
      const line = list.get(i)
      if (line.kind === "speech" && !line.mine && !names.includes(line.name))
        names.push(line.name)
    }
    return names
  }
  // Tags are one whatever their case.
  function sameTag(a, b) { return a.toLowerCase() === b.toLowerCase() }
  function tagSession(id, tag) { send("session.tag " + JSON.stringify({ id: id, tag: tag })) }
  function untagSession(id, tag) { send("session.untag " + JSON.stringify({ id: id, tag: tag })) }
  function renameTag(from, to) { send("tag.rename " + JSON.stringify({ from: from, to: to })) }
  function deleteTag(tag) { send("tag.delete " + JSON.stringify({ tag: tag })) }
  function renameSession(id, title, kind) { send("session.rename " + JSON.stringify({ id: id, title: title, kind: kind })) }
  // leaveSession leaves the session on screen — a live one keeps recording — and
  // goes back to the sessions as they were searched and filtered, where the live
  // ones are a filter away.
  function leaveSession() {
    stored = null
    timeline.clear()
    shown = ""
    openHistory()
  }
  // The live sessions this window showed, the newest last.
  property var visited: []
  // follow keeps the window on the live session it shows: a new one, or a stored
  // one resumed, brings its timeline, and the daemon learns which session the
  // window shows, so a window opened later shows a live session none shows. When
  // the one on screen ends, the one this window showed before it shows again, or none.
  function follow() {
    const now = session ? session.id : ""
    if (now === followed)
      return
    if (now === "" && followed !== "" && shown === followed) {
      visited = visited.filter(id => id !== followed && live.some(item => item.id === id))
      const back = visited.length > 0 ? visited[visited.length - 1] : ""
      shown = back
      if (back !== "")
        return
    }
    if (now !== "")
      visited = visited.filter(id => id !== now).concat([now])
    followed = now
    partials = []
    if (now !== "") {
      stored = null
      timeline.clear()
      go("start")
      send("session.timeline " + now)
    } else if (onScreen === null) {
      timeline.clear()
    }
    if (window)
      send("window.show " + JSON.stringify({ window: window, session: now }))
  }
  // fill adds a timeline's lines, notes and answers to `list`; a live one's
  // answers still streaming keep streaming.
  function fill(list, entries, sessionId) {
    for (const entry of entries) {
      if (entry.type === "transcript")
        addSpeech(entry.who, entry.name, entry.text, entry.at, list, entry.translation)
      else if (entry.type === "note")
        addNote(entry.id, entry.text, entry.at, list)
      else {
        const streaming = sessionId !== "" && !entry.done
        addSuggestion(entry.id, entry.action, entry.model, entry.prompt, entry.text, streaming, entry.at, list, entry.draft, entry.translation)
        if (streaming)
          answerStarted(entry.id, sessionId, entry.action, entry.at)
      }
    }
  }
  // setLanguage transcribes a session in `code` from now on.
  function setLanguage(sessionId, code) { send("session.language " + JSON.stringify({ id: sessionId, language: code })) }
  function requestConfig() { send("config") }
  function requestDevices() { send("devices") }
  function saveConfig(draft) { send("config.set " + JSON.stringify(Object.assign({ token: token }, draft))) }
  // requestModels lists a provider's models, with its key from `section`.
  function requestModels(target, section) {
    if (!connected)
      return
    modelsLoading = Object.assign({}, modelsLoading, { [target]: true })
    modelsAsked = Object.assign({}, modelsAsked, { [target]: true })
    send("models " + JSON.stringify({
      target: target, base_url: section.base_url || "",
      api_key_env: section.api_key_env || "", api_key_omapass: section.api_key_omapass || ""
    }))
  }
  function requestOmapass() { send("omapass") }
  // clearModels forgets a provider's model list: it belonged to another provider.
  function clearModels(target) {
    const found = Object.assign({}, models)
    const errors = Object.assign({}, modelsError)
    found[target] = []
    errors[target] = ""
    models = found
    modelsError = errors
    modelsAsked = Object.assign({}, modelsAsked, { [target]: false })
    modelsLoading = Object.assign({}, modelsLoading, { [target]: false })
  }

  // tell shows translated parts: tell([["status.saved"]]) or with arguments.
  function tell(parts, isError) {
    notice = parts.map(part => ({ key: part[0], args: part[1] || {} }))
    messageIsError = isError
    noticeCount += 1
  }

  // say shows text as it is: what the daemon wrote, or nothing ("").
  function say(text, isError) {
    notice = text ? [{ text: text }] : []
    messageIsError = isError
    noticeCount += 1
  }

  // kindName labels a kind: the four built in read in the interface language,
  // the user's own as they wrote them.
  function kindName(kind) {
    return I18n.has("kind." + kind) ? I18n.t("kind." + kind) : kind
  }

  // stateName labels a session's state: recording, paused, ended, interrupted
  // or importing.
  function stateName(state) {
    return state === "importing" ? I18n.t("import.progress") : I18n.t("session." + state)
  }

  // titleOf names a session: its title, or its kind when it has none.
  function titleOf(session) {
    return session.title || kindName(session.kind)
  }

  // languageName labels a code with the language's own name, as Qt knows it.
  function languageName(code) {
    if (code === "auto")
      return I18n.t("language.auto")
    const name = Qt.locale(code).nativeLanguageName
    return code.toUpperCase() + (name && Qt.locale(code).name !== "C" ? " · " + name : "")
  }

  // inputColor is the colour chosen for a captured input, or one from the palette
  // by its position; a device that is not captured has none and reads neutral.
  function inputColor(id) {
    const index = inputs.findIndex(input => input.id === id)
    if (index < 0)
      return Theme.dim
    return inputs[index].color || Theme.inputPalette[index % Theme.inputPalette.length]
  }

  // deviceLabel names the system defaults in the interface language.
  function deviceLabel(id, label) {
    if (id === "@default-input")
      return I18n.t("device.default_input")
    if (id === "@default-output")
      return I18n.t("device.default_output")
    return label
  }

  function isUser(name) {
    return participants.some(participant => participant.user && (participant.name === name || name.startsWith(participant.name + "#")))
  }

  function participantIndex(name) {
    return participants.findIndex(participant => participant.name === name)
  }

  function hashOf(key) {
    let hash = 0
    for (const c of key)
      hash = (hash * 31 + c.codePointAt(0)) >>> 0
    return hash
  }

  function identityColor(key, chosen) {
    if (chosen)
      return Theme.readable(chosen, Theme.background, Theme.foreground)
    return Theme.inputPalette[hashOf(key) % Theme.inputPalette.length]
  }

  function personColor(id) {
    return personColors[id] || identityColor(id, "")
  }

  // isUnknown says a person id names no one eco keeps.
  function isUnknown(id) {
    return peopleHeard && !people.some(p => p.id === id)
  }

  // personName names a person as eco keeps them, else as the session's speaker
  // linked to them was named, else as unknown; never by id.
  function personName(id, sessionId) {
    const person = people.find(p => p.id === id)
    if (person)
      return person.name
    const speaker = speakersOf(sessionId).find(s => s.person === id)
    return speaker && speaker.name ? speaker.name : I18n.t("person.unknown")
  }

  function speakerColor(sessionId, label, name) {
    const speaker = speakersOf(sessionId).find(s => s.label === label)
    if (speaker && speaker.person)
      return personColor(speaker.person)
    if (!speaker || !speaker.color) {
      const index = participantIndex(label)
      if (index >= 0)
        return Theme.participantColor(index)
    }
    return identityColor(sessionId + ":" + (label || name), speaker ? speaker.color : "")
  }

  // Streamed text waiting to reach the timeline, by suggestion and field ("text",
  // "thinking", "draft"): a model may send many small pieces a second, and each
  // change lays the timeline out again, so they are applied in batches.
  property var streamed: ({})
  function stream(id, field, piece) {
    const pending = streamed[id] || (streamed[id] = {})
    pending[field] = (pending[field] || "") + piece
    if (!streamFlush.running)
      streamFlush.start()
  }
  // Apply the batched text now; an answer that ends, fails or goes is flushed first.
  function flushStream() {
    streamFlush.stop()
    const batch = streamed
    streamed = ({})
    for (const id in batch) {
      const found = locate(id)
      if (!found)
        continue
      const row = found.list.get(found.index)
      for (const field in batch[id])
        found.list.setProperty(found.index, field, row[field] + batch[id][field])
    }
  }

  // locate finds a suggestion in the open session's timeline or the one read back.
  // answering names the action of the newest answer a session streams or waits
  // to, or "".
  function answering(sessionId) {
    let newest = null
    for (const id in answers)
      if (answers[id].session === sessionId && (!newest || answers[id].at >= newest.at))
        newest = answers[id]
    return newest ? newest.action : ""
  }
  function answerStarted(id, sessionId, action, at) {
    answers = Object.assign({}, answers, { [id]: { session: sessionId, action: action, at: at } })
  }
  // answerGone forgets an answer that ended, failed or was removed.
  function answerGone(id) {
    if (!(id in answers))
      return
    const next = Object.assign({}, answers)
    delete next[id]
    answers = next
  }

  function locate(id) {
    for (let i = timeline.count - 1; i >= 0; i--)
      if (timeline.get(i).key === id)
        return { list: timeline, index: i }
    return null
  }

  // speaking keeps what `who` is saying now; an empty text drops it.
  function speaking(who, name, text) {
    const others = partials.filter(partial => partial.who !== who)
    partials = text ? others.concat([{ who: who, name: name, text: text }]) : others
  }

  // timelineOf is where a session's answers go: the timeline when it is on screen,
  // or nowhere.
  function timelineOf(sessionId) {
    return onScreen !== null && onScreen.id === sessionId ? timeline : null
  }

  // addSpeech adds a line: `who` is the label its speaker carries, `name` what
  // the speaker goes by in that session.
  function addSpeech(who, name, text, at, target, translation) {
    const list = target
    list.append({
      kind: "speech", key: "", who: who, name: name, text: text, mine: isUser(who), at: at,
      stamp: Qt.formatTime(new Date(at * 1000), "hh:mm:ss"),
      action: "", modelName: "", prompt: "", streaming: false, thinking: "", failure: "", draft: "", hook: "",
      translation: translation || "", timing: ""
    })
  }

  // `draft` is the unreviewed answer a reviewer rewrote, kept only to inspect;
  // `hook` is how sending it to its action's hook went: "", "sending", "sent" or "failed";
  // `translation` is the answer in the session's translation language.
  function addSuggestion(id, action, model, prompt, text, streaming, at, target, draft, translation) {
    target.append({
      kind: "suggestion", key: id, who: "", name: "", text: text, mine: false, at: at,
      stamp: Qt.formatTime(new Date(at * 1000), "hh:mm:ss"),
      action: action, modelName: model, prompt: prompt || "", streaming: streaming, thinking: "", failure: "", draft: draft || "", hook: "",
      translation: translation || "", timing: ""
    })
  }

  function addNote(id, text, at, target) {
    target.append({
      kind: "note", key: id, who: "", name: "", text: text, mine: true, at: at,
      stamp: Qt.formatTime(new Date(at * 1000), "hh:mm:ss"),
      action: "", modelName: "", prompt: "", streaming: false, thinking: "", failure: "", draft: "", hook: "",
      translation: "", timing: ""
    })
  }

  // signalFor gives the InputSignal of an input, creating it on first use.
  function signalFor(id) {
    if (!signals[id]) {
      const next = Object.assign({}, signals)
      next[id] = signalModel.createObject(root)
      signals = next
    }
    return signals[id]
  }

  function handle(event) {
    switch (event.type) {
    case "snapshot":
      actions = event.actions
      hooks = event.hooks || []
      participants = event.participants
      inputs = event.inputs
      language = event.language
      languages = event.languages
      languageCodes = event.language_codes
      kinds = event.kinds
      kindModels = event.kind_models || {}
      contexts = event.contexts || []
      uiLanguage = event.ui_language
      transcribers = event.transcribers || []
      answers = ({})
      // A daemon met again sends the timeline again.
      followed = ""
      live = event.live || []
      follow()
      send("people")
      send("tags")
      if (opening) {
        act(JSON.parse(opening))
        opening = ""
      }
      break
    case "session":
      live = event.live || []
      transcribers = event.transcribers || []
      // The rows follow the sessions started, paused, resumed or ended.
      if (view === "history")
        send("sessions")
      // Nothing is being said into a session that does not record.
      if (session !== null && session.state !== "recording")
        partials = []
      break
    case "session_opened":
      if (event.window === window)
        shown = event.id
      break
    case "window_call":
      if (event.window === window)
        act(event)
      break
    case "session_timeline":
      if (session !== null && event.session === session.id) {
        timeline.clear()
        fill(timeline, event.timeline, session.id)
      }
      break
    case "sessions":
      sessions = event.sessions
      sessionsHeard = true
      break
    case "sessions_found":
      if (event.query === historySearch) {
        historyFound = event.ids
        historyAnswered = event.query
      }
      break
    case "import_date":
      importDate = { path: event.path, at: event.at }
      break
    case "import_started":
      importing = { id: event.session.id, title: event.session.title, kind: event.session.kind, done_s: 0, total_s: event.total_s }
      break
    case "import_progress":
      if (importing !== null && importing.id === event.id)
        importing = Object.assign({}, importing, { done_s: event.done_s })
      break
    case "import_done":
      importing = null
      // A session read back while it was imported has now ended.
      if (stored !== null && stored.id === event.id)
        stored = Object.assign({}, stored, { state: "ended" })
      // A failed import already showed its error.
      if (event.complete)
        tell([["status.imported"]], false)
      else if (!messageIsError)
        tell([["status.import_stopped"]], false)
      if (view === "history")
        send("sessions")
      break
    case "session_export":
      if (event.id === exporting) {
        exporting = ""
        copy(event.text, "status.transcript_copied")
      }
      break
    case "speaker_renamed": {
      const list = timelineOf(event.session)
      for (let i = 0; list && i < list.count; i++) {
        const line = list.get(i)
        if (line.kind === "speech" && line.who === event.label)
          list.setProperty(i, "name", event.name)
      }
      break
    }
    case "diarized": {
      // The session's lines, in order, now carry the speakers told apart.
      const list = timelineOf(event.session)
      let index = 0
      for (let i = 0; list && i < list.count; i++) {
        if (list.get(i).kind !== "speech")
          continue
        list.setProperty(i, "who", event.who[index])
        list.setProperty(i, "name", event.names[index])
        list.setProperty(i, "mine", isUser(event.who[index]))
        index++
      }
      requestSpeakers(event.session)
      break
    }
    case "session_context": {
      const chosen = item => item && item.id === event.session ? Object.assign({}, item, { contexts: event.contexts }) : item
      live = live.map(chosen)
      stored = chosen(stored)
      sessions = sessions.map(chosen)
      break
    }
    case "session_cost":
      if (event.session === costOf)
        cost = event.cost
      break
    case "tags":
      tags = event.tags
      break
    case "session_tags": {
      const tagged = item => item && item.id === event.session ? Object.assign({}, item, { tags: event.tags }) : item
      stored = tagged(stored)
      sessions = sessions.map(tagged)
      break
    }
    // The session read back follows a tag renamed or deleted; the list and
    // the live ones come again.
    case "tag_renamed": {
      if (stored !== null && event.sessions.includes(stored.id))
        stored = Object.assign({}, stored, { tags: stored.tags.map(tag => sameTag(tag, event.from) || sameTag(tag, event.to) ? event.to : tag)
          .filter((tag, i, all) => all.indexOf(tag) === i) })
      tagRenamed(event.from, event.to)
      break
    }
    case "tag_deleted":
      if (stored !== null && event.sessions.includes(stored.id))
        stored = Object.assign({}, stored, { tags: stored.tags.filter(tag => !sameTag(tag, event.tag)) })
      break
    case "session_renamed": {
      const renamed = session => session.id === event.id ? Object.assign({}, session, { title: event.title, kind: event.kind }) : session
      sessions = sessions.map(renamed)
      if (stored !== null)
        stored = renamed(stored)
      break
    }
    case "session_detail":
      keepSpeakers(event.session.id, event.speakers || [])
      if (event.session.id !== shown || session !== null)
        break
      stored = event.session
      timeline.clear()
      fill(timeline, event.timeline, "")
      break
    case "session_deleted":
      // Its tags count one session less.
      if ((sessions.find(item => item.id === event.id)?.tags || []).length > 0)
        send("tags")
      sessions = sessions.filter(item => item.id !== event.id)
      if (stored !== null && stored.id === event.id)
        leaveSession()
      tell([["status.session_deleted"]], false)
      break
    case "transcript": {
      // Several sessions may record at once; a line goes to the one it was heard in.
      const list = timelineOf(event.session)
      if (list)
        addSpeech(event.who, event.name, event.text, event.at, list, event.translation)
      if (session !== null && event.session === session.id)
        speaking(event.who, "", "")
      break
    }
    case "transcript_partial":
      if (session !== null && event.session === session.id)
        speaking(event.who, event.name, event.text)
      break
    case "transcript_edited": {
      const list = timelineOf(event.session)
      for (let i = 0; list && i < list.count; i++) {
        const line = list.get(i)
        if (line.kind === "speech" && line.who === event.who && Math.abs(line.at - event.at) < 1e-6) {
          list.setProperty(i, "text", event.text)
          break
        }
      }
      break
    }
    case "transcript_reassigned": {
      const list = timelineOf(event.session)
      for (let i = 0; list && i < list.count; i++) {
        const line = list.get(i)
        if (line.kind === "speech" && line.who === event.who && Math.abs(line.at - event.at) < 1e-6) {
          list.setProperty(i, "who", event.label)
          list.setProperty(i, "name", event.name)
          break
        }
      }
      break
    }
    case "transcript_removed": {
      // A line the user did not say: the others' audio, heard by the microphone.
      const list = timelineOf(event.session)
      for (let i = 0; list && i < list.count; i++) {
        const line = list.get(i)
        if (line.kind === "speech" && line.who === event.who && Math.abs(line.at - event.at) < 1e-6) {
          list.remove(i)
          break
        }
      }
      break
    }
    case "session_speakers":
      keepSpeakers(event.session, event.speakers)
      break
    case "people":
      people = event.people
      peopleHeard = true
      break
    case "attendees_changed": {
      const changed = item => item && item.id === event.session ? Object.assign({}, item, { people: event.people, attendees: event.attendees }) : item
      live = live.map(changed)
      stored = changed(stored)
      break
    }
    case "note": {
      const list = timelineOf(event.session)
      if (list)
        addNote(event.id, event.text, event.at, list)
      break
    }
    case "note_removed":
    case "suggestion_removed": {
      flushStream()
      // A failed answer stays on screen with its reason until the user closes it.
      const found = locate(event.id)
      if (found && !found.list.get(found.index).failure)
        found.list.remove(found.index)
      answerGone(event.id)
      break
    }
    case "suggestion_thinking":
      stream(event.id, "thinking", event.text)
      break
    case "suggestion_draft":
      stream(event.id, "draft", event.text)
      break
    case "signal":
      signalFor(event.input).push(event.level, event.speech)
      break
    case "suggestion_start": {
      const list = timelineOf(event.session)
      if (list)
        addSuggestion(event.id, event.action, event.model, event.prompt, "", true, event.at, list)
      answerStarted(event.id, event.session, event.action, event.at)
      if (list)
        say("", false)
      break
    }
    case "suggestion_delta":
      stream(event.id, "text", event.text)
      break
    case "suggestion_end": {
      flushStream()
      const found = locate(event.id)
      // How long it took and its cache share go to its card, read on hover.
      if (found) {
        found.list.setProperty(found.index, "streaming", false)
        found.list.setProperty(found.index, "timing", JSON.stringify({
          ttft: event.ttft_ms, total: event.total_ms,
          cache: event.prompt_tokens ? Math.round(100 * event.cached_tokens / event.prompt_tokens) : -1
        }))
      }
      answerGone(event.id)
      break
    }
    case "translated": {
      // A line is found by when it was heard, an answer by its id.
      const list = timelineOf(event.session)
      for (let i = 0; list && i < list.count; i++) {
        const row = list.get(i)
        const match = event.id ? row.key === event.id : row.kind === "speech" && Math.abs(row.at - event.at) < 1e-6
        if (match) {
          list.setProperty(i, "translation", event.text)
          break
        }
      }
      break
    }
    case "session_translation": {
      const chosen = item => item && item.id === event.session ? Object.assign({}, item, { translating: event.translating }) : item
      live = live.map(chosen)
      stored = chosen(stored)
      // Translations in another language, or none, no longer show; those kept in
      // the new one arrive right after, the rest as they are made.
      const list = timelineOf(event.session)
      for (let i = 0; list && i < list.count; i++)
        list.setProperty(i, "translation", "")
      break
    }
    case "hook_started":
    case "hook_sent": {
      const found = locate(event.id)
      if (found)
        found.list.setProperty(found.index, "hook", event.type === "hook_sent" ? "sent" : "sending")
      break
    }
    case "config":
      // Presets first: the settings draft is made from the config.
      presets = event.presets
      config = event.config
      devices = event.devices
      omapassInstalled = event.omapass.installed
      omapassPage = event.omapass.page
      break
    case "devices":
      devices = event.devices
      break
    case "config_pending":
      pendingConfig = event.hooks.length + event.files.length + event.models.length > 0 ? event : null
      break
    case "config_saved":
      tell([["status.saved"]], false)
      configSaveSucceeded()
      break
    case "omapass":
      omapassAccounts = event.accounts
      omapassError = event.error || ""
      omapassInstalled = event.installed
      break
    case "models": {
      const found = Object.assign({}, models)
      const errors = Object.assign({}, modelsError)
      found[event.target] = event.models
      // A coded failure reads in the interface language; others as the provider wrote them.
      errors[event.target] = event.code && I18n.has("error." + event.code)
        ? I18n.t("error." + event.code, event.params) : event.error || ""
      models = found
      modelsError = errors
      modelsLoading = Object.assign({}, modelsLoading, { [event.target]: false })
      break
    }
    case "error": {
      flushStream()
      const about = event.params && event.params.session
      const answer = event.code === "completion.failed" && event.params && event.params.id
      const failed = answer ? locate(answer) : null
      if (failed) {
        failed.list.setProperty(failed.index, "streaming", false)
        failed.list.setProperty(failed.index, "failure", event.params.detail || event.message)
      }
      if (answer)
        answerGone(answer)
      // A hook fails apart from any answer streaming meanwhile.
      if (event.code === "hook.failed" && event.params) {
        const found = locate(event.params.id)
        if (found)
          found.list.setProperty(found.index, "hook", "failed")
      }
      if (event.code === "config.invalid")
        configSaveFailed((event.params && event.params.detail) || event.message || "")
      // An error about one session or answer shows only in the window showing it.
      if (about ? onScreen === null || onScreen.id !== about : answer && failed === null)
        break
      // A coded error reads in the interface language; others as the daemon wrote them.
      if (event.code && I18n.has("error." + event.code))
        tell([["error." + event.code, event.params]], true)
      else
        say(event.message, true)
      break
    }
    }
  }

  Timer {
    interval: 1000
    repeat: true
    triggeredOnStart: true
    running: root.live.some(item => item.running_since !== null)
    onTriggered: root.clock = Date.now() / 1000
  }

  Timer {
    id: streamFlush
    interval: 60
    onTriggered: root.flushStream()
  }

  Component {
    id: signalModel
    InputSignal {}
  }

  // The connection to the daemon, made again every second while it (which may
  // restart) does not answer.
  LineSocket {
    id: link
    path: Host.env("XDG_RUNTIME_DIR") + "/eco.sock"
    onReceived: line => root.handle(JSON.parse(line))
  }
}
