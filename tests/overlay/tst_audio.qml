import QtQuick
import QtTest
import "../../overlay/Eco/Core/draft.js" as Draft

// An audio source's devices have one owner, one source at most is the user,
// and removing one says what stops being heard.
TestCase {
  name: "Audio"

  function draft() {
    return { participants: [
      { name: "Me", devices: ["@default-input"], user: true },
      { name: "Them", devices: ["@default-output", "usb-out"], user: false },
      { name: "Room", devices: [], user: false }
    ] }
  }

  readonly property var devices: [
    { id: "@default-input", kind: "input" },
    { id: "@default-output", kind: "output" },
    { id: "usb-out", kind: "output" }
  ]

  function test_assigning_a_device_takes_it_from_its_owner() {
    const d = draft()
    Draft.assign(d, "usb-out", 2)
    compare(d.participants.map(person => person.devices), [["@default-input"], ["@default-output"], ["usb-out"]])
  }

  function test_unassigning_a_device_leaves_it_to_nobody() {
    const d = draft()
    Draft.assign(d, "@default-input", -1)
    compare(d.participants.map(person => person.devices), [[], ["@default-output", "usb-out"], []])
  }

  function test_one_source_at_most_is_the_user() {
    const d = draft()
    Draft.makeUser(d, 1)
    compare(d.participants.map(person => person.user), [false, true, false])
    Draft.makeUser(d, 1)
    compare(d.participants.map(person => person.user), [false, false, false])
  }

  function test_removing_the_last_source_with_an_output_says_so() {
    compare(Draft.sourceRemoval(draft(), 1, devices), { unheard: ["@default-output", "usb-out"], noOutput: true, noUser: false })
  }

  function test_removing_the_user_says_so() {
    compare(Draft.sourceRemoval(draft(), 0, devices), { unheard: ["@default-input"], noOutput: false, noUser: true })
  }

  function test_another_source_with_an_output_keeps_the_others_heard() {
    const d = draft()
    Draft.assign(d, "usb-out", 2)
    compare(Draft.sourceRemoval(d, 1, devices).noOutput, false)
  }
}
