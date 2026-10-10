eco keeps voice sessions: meetings, conversations, spoken ideas. It captures
the audio, transcribes it, and streams short answers from a model into a
window, live or afterwards.

This folder is the whole program. Keep it where it is, for example
%LOCALAPPDATA%\Programs\eco, and add its bin folder to your PATH.

    eco setup             download the models and start eco at login
    eco start             start eco and open its window
    eco status            say whether eco runs
    eco stop              stop it

eco reads its config from %APPDATA%\eco\config.toml. API keys come from
environment variables, for example:

    setx OPENROUTER_API_KEY "<your key>"

(<your key> is a placeholder.) Open a new terminal after setx.

Needs Windows 10 or later, 64-bit, with the Microsoft Visual C++
Redistributable (x64).

Docs: https://github.com/This-Is-NPC/eco/blob/master/docs/how-to-install-and-remove.md#on-windows
Licence: LICENSE (MIT).
