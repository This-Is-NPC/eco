import QtQuick
import QtTest
import "../../overlay/Eco/Core/draft.js" as Draft

// The offered spoken languages are added once and removed by code; the
// default removed passes to the first one left, and the draft still saves.
TestCase {
  name: "Transcription"

  function draft() {
    return { participants: [], models: [], contexts: [], actions: [], kinds: ["meeting"],
      stt: { model: "", language: "pt", languages: ["en", "pt", "auto"] }, llm: { model: "" } }
  }

  function test_a_language_is_offered_once_and_last() {
    const d = draft()
    Draft.addLanguage(d, "ja")
    Draft.addLanguage(d, "en")
    compare(d.stt.languages, ["en", "pt", "auto", "ja"])
    compare(Draft.problems(d), [])
  }

  function test_removing_another_language_keeps_the_default() {
    const d = draft()
    compare(Draft.languageHeir(d, "en"), "")
    Draft.removeLanguage(d, "en")
    compare([d.stt.languages, d.stt.language], [["pt", "auto"], "pt"])
    compare(Draft.problems(d), [])
  }

  function test_removing_the_default_makes_the_first_left_the_default() {
    const d = draft()
    compare(Draft.languageHeir(d, "pt"), "en")
    Draft.removeLanguage(d, "pt")
    compare([d.stt.languages, d.stt.language], [["en", "auto"], "en"])
    compare(Draft.problems(d), [])
    d.stt.language = "en"
    compare(Draft.languageHeir(d, "en"), "auto")
  }
}
