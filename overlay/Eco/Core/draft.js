.pragma library

// Reads and edits of the settings draft that several views share.

// The names of the draft's `list` ("secrets", "providers" or "models").
function listed(draft, list) {
  return draft[list].map(item => item.name)
}

// The names of the registered models of `type`, "chat" or "transcription".
function named(draft, type) {
  return draft.models.filter(model => model.type === type && model.name).map(model => model.name)
}

// Whether the draft has a chat model registered: answers, skills and
// translation need one.
function chats(draft) {
  return named(draft, "chat").length > 0
}

// The model sessions of `kind` choose for `use` — "transcription", "chat" or
// "translation" — or "" when they keep the default.
function serving(draft, use, kind) {
  const own = draft.models.find(model => use === "translation" ? (model.translates || []).includes(kind)
    : model.type === use && (model.kinds || []).includes(kind))
  return own ? own.name : ""
}

// Make sessions of `kind` use the model `name` for `use` in the draft `d`, or the default for "".
function serve(d, use, kind, name) {
  const field = use === "translation" ? "translates" : "kinds"
  const type = use === "transcription" ? "transcription" : "chat"
  d.models.forEach(model => {
    if (model.type !== type)
      return
    const kinds = (model[field] || []).filter(k => k !== kind)
    if (model.name === name)
      kinds.push(kind)
    if (kinds.length)
      model[field] = kinds
    else
      delete model[field]
  })
}

// The model that really answers, as the daemon picks it: for `use` —
// "chat", "transcription" or "translation" — of the skill `action` (an entry
// of the draft's actions, or null) in a session of `kind` ("" for any). A chat
// answer takes the skill's model, then the kind's, then the assistant's; a
// translation, the kind's, the translation default, then the assistant's; a
// transcription, the kind's, then the default. Returns {name, own}, `own`
// when the most specific of the skill, the kind and the default asked names it
// itself rather than inheriting it.
function answering(draft, use, kind, action) {
  const type = use === "transcription" ? "transcription" : "chat"
  const known = name => !!name && named(draft, type).includes(name)
  const steps = []
  if (action)
    steps.push(action.model || "")
  if (kind)
    steps.push(serving(draft, use, kind))
  if (use === "translation")
    steps.push(draft.translation && draft.translation.model || "")
  steps.push(use === "transcription" ? draft.stt.model : draft.llm.model)
  const at = steps.findIndex(known)
  return { name: at < 0 ? "" : steps[at], own: at === 0 }
}

// Whether the registered model `entry` answers `use` of the skill `action` in
// a session of `kind`, as `answering` picks it; one with no name answers nothing.
function serves(draft, entry, use, kind, action) {
  return !!entry.name && answering(draft, use, kind, action).name === entry.name
}

// The draft with the model the daemon picked (`defaults`, its default_models)
// in each role left empty that the daemon lists as automatic, and those roles:
// {draft, automatic} among "transcription", "chat", "translation", "review".
function resolved(draft, defaults) {
  const next = JSON.parse(JSON.stringify(draft))
  const automatic = []
  const fill = (role, current, set) => {
    const picked = (defaults.automatic || []).includes(role) ? defaults[role] : ""
    if (!current && picked) {
      set(picked)
      automatic.push(role)
    }
  }
  fill("transcription", draft.stt.model, name => { next.stt.model = name })
  fill("chat", draft.llm.model, name => { next.llm.model = name })
  fill("translation", draft.translation && draft.translation.model, name => { next.translation = { model: name } })
  fill("review", draft.reviewer && draft.reviewer.model, name => { next.reviewer = Object.assign({ enabled: false }, next.reviewer, { model: name }) })
  return { draft: next, automatic: automatic }
}

// What the model `name` of `draft` is the default for, as the daemon picks
// it: [{use, inherited}], `use` "chat", "translation", "review" (the
// reviewer's model, on or off) or "transcription"; translation `inherited`
// when no translation default is set and it follows the chat one.
function defaults(draft, name) {
  const found = []
  const is = use => !!name && answering(draft, use, "", null).name === name
  if (is("chat"))
    found.push({ use: "chat", inherited: false })
  if (is("translation"))
    found.push({ use: "translation", inherited: !answering(draft, "translation", "", null).own })
  if (!!name && !!draft.reviewer && draft.reviewer.model === name)
    found.push({ use: "review", inherited: false })
  if (is("transcription"))
    found.push({ use: "transcription", inherited: false })
  return found
}

// Make the model `name` the draft `d`'s default for `use`: "chat",
// "translation", "review" (the reviewer's model; on or off stays) or
// "transcription".
function makeDefault(d, use, name) {
  if (use === "chat")
    d.llm.model = name
  else if (use === "transcription")
    d.stt.model = name
  else if (use === "review")
    d.reviewer = Object.assign({ enabled: false }, d.reviewer, { model: name })
  else
    d.translation = Object.assign({}, d.translation, { model: name })
}

// Name the model at `index` in the draft `d` as `text` is typed. The defaults,
// reviewer, translation and skills that named it name it `from`, the last name
// it had that was nonempty and no other model's ("" for a model never named,
// which none of them names); they follow the typed name, trimmed, only once it
// is such a name too, so a model never takes another's. Returns the name they
// hold now.
function rename(d, index, text, from) {
  d.models[index].name = text
  const name = text.trim()
  if (!name || name === from || d.models.some((model, i) => i !== index && model.name.trim() === name))
    return from
  if (!from)
    return name
  if (d.llm.model === from) d.llm.model = name
  if (d.stt.model === from) d.stt.model = name
  if (d.reviewer && d.reviewer.model === from) d.reviewer.model = name
  if (d.translation && d.translation.model === from) d.translation.model = name
  d.actions.forEach(action => { if (action.model === from) action.model = name })
  return name
}

// The provider a new model of `type` starts from: that of the draft's last
// model of that type; "" when there is none.
function provider(draft, type) {
  const last = draft.models.filter(model => model.type === type && model.provider).pop()
  return last ? last.provider : ""
}

// Make the model at `index` in the draft `d` the other `type`, keeping its
// name, provider and model id. A chat model's reasoning, extra fields and
// translations leave with its type.
function retype(d, index, type) {
  const entry = d.models[index]
  d.models[index] = { name: entry.name, type: type, provider: entry.provider, model: entry.model || "" }
}

// "env", "omapass" or "credential": where the key of `secret` is read from.
function sourceOf(secret) {
  return secret.credential !== undefined ? "credential" : secret.omapass !== undefined ? "omapass" : "env"
}

// The pack key that names a secret `source`.
function sourceKey(source) {
  return source === "credential" ? "provider.key_credential" : source === "omapass" ? "provider.key_omapass" : "provider.key_env"
}

// Name the secret or provider at `index` of the draft's `list` ("secrets" or
// "providers") as `text` is typed. What refers to it — the providers' `secret`,
// the models' `provider` — names it `from`, the last name it had that was
// nonempty and no other item's ("" for one never named); it follows the typed
// name, trimmed, only once it is such a name too. Returns the name it holds now.
function renameItem(d, list, index, text, from) {
  d[list][index].name = text
  const name = text.trim()
  if (!name || name === from || d[list].some((item, i) => i !== index && item.name.trim() === name))
    return from
  if (!from)
    return name
  const holders = list === "secrets" ? d.providers : d.models
  const field = list === "secrets" ? "secret" : "provider"
  holders.forEach(holder => { if (holder[field] === from) holder[field] = name })
  return name
}

// The names of what refers to the secret ("secrets") or provider ("providers")
// called `name`: the draft's providers or models.
function referrers(draft, list, name) {
  const holders = list === "secrets" ? "providers" : "models"
  const field = list === "secrets" ? "secret" : "provider"
  return draft[holders].filter(holder => !!name && holder[field] === name).map(holder => holder.name)
}

// What uses the model named `name` in the draft now, as `answering` picks it:
// [{role, kind, uses, action}], `role` one of "assistant", "transcriber",
// "translation" (the translation default), "reviewer" (its model, on or off),
// "skill" (`action` its name) or "kind" (`kind` once, with its `uses`: "chat",
// "transcription" and "translation"). A model with no name, or none of that
// name, is used by nothing.
function users(draft, name) {
  const entry = draft.models.find(model => model.name === name)
  if (!name || !entry)
    return []
  const found = []
  const answers = (use, kind) => serves(draft, entry, use, kind, null)
  const chat = entry.type === "chat"
  if (chat && answers("chat", ""))
    found.push({ role: "assistant" })
  if (!chat && answers("transcription", ""))
    found.push({ role: "transcriber" })
  if (chat && answers("translation", ""))
    found.push({ role: "translation" })
  if (chat && draft.reviewer && draft.reviewer.model === name)
    found.push({ role: "reviewer" })
  draft.actions.forEach(action => { if (action.model === name) found.push({ role: "skill", action: action.name }) })
  const uses = chat ? ["chat", "translation"] : ["transcription"]
  draft.kinds.forEach(kind => {
    const served = uses.filter(use => answers(use, kind))
    if (served.length > 0)
      found.push({ role: "kind", kind: kind, uses: served })
  })
  return found
}

// Replace the kind `from` by `to` in the draft `d`'s models (`kinds`,
// `translates`) and context slots, or drop it there for a null `to`.
function followKind(d, from, to) {
  const swap = list => list.map(kind => kind === from ? to : kind).filter(kind => kind !== null)
  d.models.forEach(model => {
    for (const field of ["kinds", "translates"]) {
      if (!model[field])
        continue
      model[field] = swap(model[field])
      if (!model[field].length)
        delete model[field]
    }
  })
  ;(d.contexts || []).forEach(slot => slot.kinds = swap(slot.kinds))
}

// The edit of an item made again on `config`, the saved config that changed
// since the edit began from `base`: a copy of `config` with `make` (the new
// item, or null) and each of `changes` applied, in order. Null when a list
// the edit reached changed there — one of those the item's `paths` live in
// ("models.2" lives in "models") or any other `draft`, the edit as it is now,
// changed — since the changes made again may then fall on another item.
function rebased(config, base, draft, paths, make, changes) {
  const reached = Object.keys(draft).filter(list => JSON.stringify(draft[list]) !== JSON.stringify(base[list]))
  const lists = paths.map(path => path.split(".")[0]).concat(reached)
  if (lists.some(list => JSON.stringify(config[list]) !== JSON.stringify(base[list])))
    return null
  const next = JSON.parse(JSON.stringify(config))
  if (make)
    make(next)
  changes.forEach(change => change(next))
  return next
}

// A copy of the draft `d` as it is saved: names, model ids and the names
// that refer to models trimmed, languages trimmed and lowercase, and each
// model serving only the kinds there are.
function normalised(d) {
  const next = JSON.parse(JSON.stringify(d))
  const trim = name => name.trim()
  next.participants.forEach(person => person.name = trim(person.name))
  next.kinds = next.kinds.map(trim)
  next.stt.languages = next.stt.languages.map(code => code.trim().toLowerCase())
  next.actions.forEach(action => {
    action.name = trim(action.name)
    if (action.model)
      action.model = trim(action.model)
  })
  next.secrets.forEach(secret => {
    secret.name = trim(secret.name)
    for (const field of ["env", "omapass", "credential"])
      if (secret[field] !== undefined)
        secret[field] = trim(secret[field])
  })
  next.providers.forEach(provider => {
    provider.name = trim(provider.name)
    provider.base_url = trim(provider.base_url)
  })
  next.models.forEach(model => {
    model.name = trim(model.name)
    model.model = trim(model.model)
  })
  next.llm.model = trim(next.llm.model)
  next.stt.model = trim(next.stt.model)
  if (next.translation && next.translation.model)
    next.translation.model = trim(next.translation.model)
  if (next.reviewer)
    next.reviewer.model = trim(next.reviewer.model)
  next.contexts = (next.contexts || []).map(slot => Object.assign(slot, { name: trim(slot.name) }))
  next.models.forEach(model => {
    for (const field of ["kinds", "translates"]) {
      if (model[field])
        model[field] = model[field].filter(kind => next.kinds.includes(kind))
      if (model[field] && !model[field].length)
        delete model[field]
    }
  })
  return next
}

// Remove the model at `index` from the draft `d`: the skills and translation
// default that named it fall back, and a reviewer on it turns off.
function removeModel(d, index) {
  const name = d.models[index].name
  d.models.splice(index, 1)
  if (!name)
    return
  if (d.reviewer && d.reviewer.model === name) {
    d.reviewer.model = ""
    d.reviewer.enabled = false
  }
  if (d.translation && d.translation.model === name)
    delete d.translation.model
  d.actions.forEach(action => { if (action.model === name) delete action.model })
}

// What removing the model at `index` changes: `moves`, each a skill (`action`,
// its name) or a kind's `use` of `kind` ("" for the translation default) with
// the model that answers it `to` afterwards, as `answering` picks it; and
// `reviewerOff`, when the reviewer ran on it.
function removal(draft, index) {
  const entry = draft.models[index]
  const moves = []
  if (!entry.name)
    return { moves: moves, reviewerOff: false }
  const after = JSON.parse(JSON.stringify(draft))
  removeModel(after, index)
  const move = (use, kind, action) => moves.push({ use: use, kind: kind, action: action ? action.name : "",
    to: answering(after, use, kind, action).name })
  draft.actions.forEach((action, i) => { if (action.model === entry.name) move("chat", "", after.actions[i]) })
  ;(entry.kinds || []).forEach(kind => move(entry.type, kind, null))
  ;(entry.translates || []).forEach(kind => move("translation", kind, null))
  if (draft.translation && draft.translation.model === entry.name)
    move("translation", "", null)
  return { moves: moves, reviewerOff: !!draft.reviewer && !!draft.reviewer.enabled && draft.reviewer.model === entry.name }
}

// What keeps the draft `d` from saving, each on the field it concerns, in the
// order of the settings tabs: {path, key, typed}. `path` names the field
// ("actions.2", "models.0.model"), `key` its message; `typed` marks what is
// wrong in what was typed — a repeated name, a space — rather than missing.
// Names compare trimmed, as they are saved.
function problems(d) {
  const found = []
  const add = (path, key, typed) => found.push({ path: path, key: key, typed: !!typed })
  // Every name nonempty and unique; with `spaced` false, also without spaces.
  const names = (list, path, spaced) => {
    const trimmed = list.map(name => name.trim())
    trimmed.forEach((name, i) => {
      if (!name)
        add(path(i), "settings.error_empty", false)
      else if (!spaced && /\s/.test(name))
        add(path(i), "settings.error_spaces", true)
      else if (trimmed.filter(other => other === name).length > 1)
        add(path(i), "settings.error_taken", true)
    })
  }
  const typed = type => named(d, type).map(name => name.trim())
  names(d.participants.map(person => person.name), i => "participants." + i, true)
  names(d.secrets.map(secret => secret.name), i => "secrets." + i + ".name", true)
  d.secrets.forEach((secret, i) => {
    const given = ["env", "omapass", "credential"].filter(field => (secret[field] || "").trim() !== "")
    if (given.length !== 1)
      add("secrets." + i + ".source", "settings.error_secret_source")
    else if (given[0] === "credential" && /\s/.test(secret.credential.trim()))
      add("secrets." + i + ".source", "settings.error_credential_name", true)
  })
  names(d.providers.map(provider => provider.name), i => "providers." + i + ".name", true)
  d.providers.forEach((provider, i) => {
    if (!provider.base_url.trim())
      add("providers." + i + ".base_url", "settings.error_base_url")
    if (provider.secret && !listed(d, "secrets").includes(provider.secret))
      add("providers." + i + ".secret", "settings.error_secret_unknown")
  })
  names(d.models.map(model => model.name), i => "models." + i + ".name", true)
  d.models.forEach((model, i) => {
    if (!listed(d, "providers").includes(model.provider))
      add("models." + i + ".provider", "settings.error_provider_pick")
    if (!model.model.trim())
      add("models." + i + ".model", "settings.error_model_id")
  })
  const languages = d.stt.languages.map(code => code.trim().toLowerCase())
  if (!languages.length || languages.some(code => code !== "auto" && !/^[a-z]{2,3}$/.test(code))
      || new Set(languages).size !== languages.length || !languages.includes(d.stt.language))
    add("stt.languages", "settings.invalid_languages")
  // A default may name no model, as before any is registered; one it names is registered.
  if (d.stt.model.trim() && !typed("transcription").includes(d.stt.model.trim()))
    add("stt.model", "settings.invalid_transcriber")
  if (d.llm.model.trim() && !typed("chat").includes(d.llm.model.trim()))
    add("llm.model", "settings.invalid_assistant")
  names((d.contexts || []).map(slot => slot.name), i => "contexts." + i, true)
  if (d.reviewer && d.reviewer.enabled && !typed("chat").includes(d.reviewer.model.trim()))
    add("reviewer.model", "settings.invalid_reviewer")
  names(d.actions.map(action => action.name), i => "actions." + i, false)
  names(d.kinds, i => "kinds." + i, true)
  return found
}

// Give the device `id` to the audio source at `index` of the draft `d`,
// taking it from whoever had it; with `index` -1 nobody is heard through it.
function assign(d, id, index) {
  d.participants.forEach(person => person.devices = person.devices.filter(device => device !== id))
  if (index >= 0)
    d.participants[index].devices.push(id)
}

// Make the audio source at `index` of the draft `d` the user, the only one;
// when it was already, nobody is.
function makeUser(d, index) {
  const was = d.participants[index].user
  d.participants.forEach((person, i) => person.user = i === index && !was)
}

// What removing the audio source at `index` of `draft` loses, given the
// `devices` there are ({id, kind}): the devices it was heard through,
// `noOutput` when no other source has an output device, `noUser` when it was the user.
function sourceRemoval(draft, index, devices) {
  const source = draft.participants[index]
  const outputs = person => person.devices.some(id => devices.some(device => device.id === id && device.kind === "output"))
  return { unheard: source.devices.slice(), noOutput: outputs(source) && !draft.participants.some((person, i) => i !== index && outputs(person)),
    noUser: !!source.user }
}

// Offer the spoken language `code` in the draft `d`, last, unless it is offered.
function addLanguage(d, code) {
  if (!d.stt.languages.includes(code))
    d.stt.languages.push(code)
}

// The language that becomes the default when `code` is no longer offered in
// `draft`: the first one left when `code` was the default, else "".
function languageHeir(draft, code) {
  return draft.stt.language === code ? draft.stt.languages.filter(other => other !== code)[0] || "" : ""
}

// Stop offering the spoken language `code` in the draft `d`; when it was the
// default, the first one left becomes the default.
function removeLanguage(d, code) {
  const heir = languageHeir(d, code)
  d.stt.languages.splice(d.stt.languages.indexOf(code), 1)
  if (heir)
    d.stt.language = heir
}

// Name the kind at `index` in the draft `d` as `text` is typed. Its models and
// context slots name it `from`, the last name it had that was nonempty and no
// other kind's; they follow the typed name, trimmed, only once it is such a name
// too, so a kind never takes another's. Returns the name they hold now.
function renameKind(d, index, text, from) {
  d.kinds[index] = text
  const name = text.trim()
  if (!name || name === from || d.kinds.some((kind, i) => i !== index && kind.trim() === name))
    return from
  followKind(d, from, name)
  return name
}

// What removing the kind `kind` from the draft changes: `heir`, the kind that
// becomes the default ("" when it was not the default), and `own`, whether
// models or context slots chose it.
function kindRemoval(draft, kind) {
  return {
    heir: draft.kinds[0] === kind ? draft.kinds[1] || "" : "",
    own: draft.models.some(model => (model.kinds || []).includes(kind) || (model.translates || []).includes(kind))
      || (draft.contexts || []).some(slot => slot.kinds.includes(kind))
  }
}

// Remove the kind `kind` from the draft `d`, its models and its context slots.
function removeKind(d, kind) {
  d.kinds.splice(d.kinds.indexOf(kind), 1)
  followKind(d, kind, null)
}

// Turn the draft `d`'s reviewer on or off; turned on with no model, it
// reviews with the assistant's.
function review(d, on) {
  const reviewer = d.reviewer || { model: "" }
  reviewer.enabled = on
  if (on && !reviewer.model)
    reviewer.model = d.llm.model
  d.reviewer = reviewer
}

// The files of a context in the draft `d`, sent in this order: the global
// context's when `slot` is "", else those of the context slot named `slot`.
function contextFiles(d, slot) {
  return slot === "" ? d.context.files : d.contexts.find(entry => entry.name === slot).files
}

// Add `paths` to the file list `list`, last, those not in it yet.
function addFiles(list, paths) {
  for (const path of paths)
    if (!list.includes(path))
      list.push(path)
}

// Stop sending the file `path` with the context `slot` of the draft `d`.
function removeFile(d, slot, path) {
  const files = contextFiles(d, slot)
  files.splice(files.indexOf(path), 1)
}

// Show the device `id` in `color` in the draft `d`, or in its automatic colour for "".
function recolor(d, id, color) {
  d.colors = d.colors || {}
  if (color)
    d.colors[id] = color
  else
    delete d.colors[id]
}

// Add the audio source `name` to the draft `d`, heard through the device `id`,
// taken from whoever had it.
function addSource(d, name, id) {
  d.participants.push({ name: name, devices: [], user: false })
  assign(d, id, d.participants.length - 1)
}

// Call the audio source named `from` of the draft `d` `to`.
function renameSource(d, from, to) {
  d.participants.find(person => person.name === from).name = to
}
