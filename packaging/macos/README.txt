eco keeps voice sessions: meetings, conversations, spoken ideas. It captures
the audio, transcribes it, and streams short answers from a model into a
window, live or afterwards.

This folder is the whole program. Keep it where it is, for example
~/Applications/eco, and add its bin folder to your PATH.

    eco setup             download the models
    eco start             start eco and open its window
    eco status            say whether eco runs
    eco stop              stop it

eco reads its config from ~/Library/Application Support/eco/config.toml,
which its settings window writes at the first save; until then eco runs with
no model. API keys come from your login session's environment, for example:

    launchctl setenv OPENROUTER_API_KEY "<your key>"

(<your key> is a placeholder.) Then run eco restart.

Needs a Mac with Apple Silicon on macOS 14.6 or later. The programs are
signed ad hoc, not by a developer: an archive downloaded in a browser is
quarantined, so run `xattr -dr com.apple.quarantine ~/Applications/eco` once.

Docs: https://github.com/This-Is-NPC/eco/blob/master/docs/how-to-install-and-remove.md#on-macos
Licence: LICENSE (MIT).
