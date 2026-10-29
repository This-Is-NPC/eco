eco keeps voice sessions: meetings, conversations, spoken ideas. It captures
the audio, transcribes it, and streams short answers from a model into a
window, live or afterwards.

This folder is the whole program, installed by the MSI in C:\Program Files\eco
with its bin folder on the PATH, the models in share\eco\models and eco in
the Start menu. Keep it whole.

    eco setup             name eco's notifications for your account
    eco start             start eco and open its window
    eco status            say whether eco runs
    eco stop              stop it

eco reads its config from %APPDATA%\eco\config.toml, which its settings
window writes at the first save; until then eco runs with no model. API keys
come from environment variables, for example:

    setx OPENROUTER_API_KEY "<your key>"

(<your key> is a placeholder.) Open a new terminal after setx.

Needs 64-bit Windows (tested on Windows 11) with the Microsoft Visual C++
Redistributable (x64).

Docs: https://github.com/This-Is-NPC/eco/blob/master/docs/how-to-install-and-remove.md#on-windows
Licence: LICENSE (MIT). The models have their own: MODELS-LICENSES.txt.
