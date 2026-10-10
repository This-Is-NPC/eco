pragma Singleton
import QtQuick
import Qt.labs.folderlistmodel
import EcoHost

// I18n holds the language packs: one JSON file per language in ./i18n, found
// on their own and reloaded when edited, so fixing or adding a translation
// never touches code. `t` looks a key up in the chosen pack, then in en-US,
// and shows the key itself when neither has it.
Singleton {
  id: root

  readonly property string fallback: "en-US"
  // code -> pack (an object of key -> text, plus "_name" and "_locale").
  property var packs: ({})
  readonly property var available: Object.keys(packs).sort().map(code => ({ code: code, name: packs[code]._name || code }))
  // The pack in use: the configured one, else the system's, else English.
  readonly property string code: resolve(Eco.uiLanguage)
  readonly property var locale: Qt.locale((packs[code] && packs[code]._locale) || code.replace("-", "_"))

  function resolve(wanted) {
    if (wanted && wanted !== "auto" && packs[wanted])
      return wanted
    const system = Qt.locale().name.replace("_", "-")
    if (packs[system])
      return system
    const sameLanguage = Object.keys(packs).find(c => c.split("-")[0] === system.split("-")[0])
    return sameLanguage || fallback
  }

  function lookup(key) {
    const chosen = packs[code]
    if (chosen && chosen[key] !== undefined)
      return chosen[key]
    const base = packs[fallback]
    return base ? base[key] : undefined
  }

  // t(key, {name: value}) fills {name} placeholders; with an `n` or `count`
  // argument it also tries `key.one` / `key.other`.
  function t(key, args) {
    const values = args || {}
    let text = lookup(key)
    const count = values.n !== undefined ? values.n : values.count
    if (text === undefined && count !== undefined)
      text = lookup(key + (count === 1 ? ".one" : ".other"))
    if (text === undefined)
      return key
    return text.replace(/\{(\w+)\}/g, (match, name) => values[name] !== undefined ? String(values[name]) : match)
  }

  function has(key) {
    return lookup(key) !== undefined || lookup(key + ".other") !== undefined
  }

  // A date in the interface language.
  function date(seconds, format) {
    return new Date(seconds * 1000).toLocaleString(locale, format)
  }

  // The day `seconds` falls on: today, yesterday, or its date, with the year
  // when it is not this one.
  function day(seconds) {
    const at = new Date(seconds * 1000)
    const midnight = new Date()
    midnight.setHours(0, 0, 0, 0)
    const start = new Date(at)
    start.setHours(0, 0, 0, 0)
    const back = Math.round((midnight - start) / 86400000)
    if (back === 0)
      return t("day.today")
    if (back === 1)
      return t("day.yesterday")
    return at.toLocaleString(locale, t(at.getFullYear() === midnight.getFullYear() ? "day.date" : "day.date_year"))
  }

  // When `seconds` was: its day and the time.
  function when(seconds) {
    return t("day.when", { day: day(seconds), time: date(seconds, t("day.time")) })
  }

  // A duration as hh:mm:ss.
  function elapsed(seconds) {
    const whole = Math.max(0, Math.floor(seconds || 0))
    const pad = value => String(value).padStart(2, "0")
    return pad(Math.floor(whole / 3600)) + ":" + pad(Math.floor(whole / 60) % 60) + ":" + pad(whole % 60)
  }

  // An amount in US$ in the interface language: cents from US$ 1 up, two
  // significant digits below. `partial` says part of it is unknown: "≥" before
  // what is known, "?" when nothing is.
  function money(usd, partial) {
    if (partial && !(usd > 0))
      return t("cost.unknown")
    const digits = usd >= 1 ? 2 : usd > 0 ? Math.max(2, 1 - Math.floor(Math.log10(usd))) : 0
    const amount = t("cost.usd", { amount: Number(usd).toLocaleString(locale, "f", digits) })
    return partial ? t("cost.at_least", { amount: amount }) : amount
  }

  property FolderListModel folder: FolderListModel {
    folder: Qt.resolvedUrl("i18n")
    nameFilters: ["*.json"]
    showDirs: false
  }

  property Instantiator loaders: Instantiator {
    model: root.folder
    delegate: TextFile {
      required property string filePath
      required property string fileBaseName
      path: filePath
      onChanged: reload()
      onTextChanged: {
        try {
          const next = Object.assign({}, root.packs)
          next[fileBaseName] = JSON.parse(text)
          root.packs = next
        } catch (error) {
          console.warn("i18n: " + fileBaseName + ".json is not valid JSON: " + error)
        }
      }
    }
  }
}
