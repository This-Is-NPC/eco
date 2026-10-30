# How to install eco, and how to take it off again

**The question:** how do I get eco onto this laptop, what does it put there, and
what is left when I remove it?

This page is enough on its own. It ends with eco installed from its package,
its window open, how it updates itself, and the one command that takes it
back off. Registering models
and choosing what eco hears are their own pages; this one only gets the program
on the machine.

---

## What it needs

- **Omarchy**, or Arch with Hyprland, PipeWire and a systemd user session. eco is
  released for Linux, and as an MSI for Windows ([on Windows](#on-windows)). The Hyprland config it ships is Lua (`o.bind`, `o.window`).
- **PipeWire's tools**: `pw-record` captures, `pw-dump` lists the devices.
- **Qt 6** (`qt6-base`, `qt6-declarative`), which draws the window. Omarchy
  already has it, and the package depends on it.
- **`ffmpeg` and `ffprobe`**, only to import audio or video files.
- **`wl-copy`**, to copy an answer or a transcript to the clipboard.
- **A key for each paid provider you use** — OpenRouter for answers, Deepgram or
  ElevenLabs for live transcription — or a model server of your own.
  [How to register models](how-to-register-models.md) is the next page.

---

## 1. Install the package

Not published as an Omarchy default yet. From the latest GitHub release:

```bash
curl -fsSL https://raw.githubusercontent.com/This-Is-NPC/eco/master/install.sh | bash
VERSION=0.1.0 bash install.sh    # a release other than the latest
```

Every GitHub release carries the Arch package and a `SHA256SUMS` beside it.
`install.sh` downloads the package, refuses it when the checksum does not match,
and installs it with `pacman -U`.

**Installing a newer release is the upgrade.** eco does it for you from the
window or with `eco update` ([Update](#update)). Run by hand, `install.sh`
does the same: `pacman -U` replaces the last
one. A daemon that was running goes on running the old binary, which cannot
serve the new window. The next `eco start`, launcher or global shortcut
restarts it on the new one first and says so on stderr:
`eco: restarted the daemon 0.1.1 → 0.2.0` (the versions here are examples).
`eco restart` does it by hand, and also reopens the window if it was open.

## 2. Download its models

```bash
eco setup --harnesses agents,claude-code
```

`eco setup` downloads the two models eco runs on the laptop — Silero VAD, which
finds speech, and WeSpeaker CAM++, which tells voices apart — checks each
against a pinned checksum, loads eco's Hyprland rules (step 4) and publishes the
`eco` skill for agents:

```
eco: saved /home/you/.local/share/eco/models/silero_vad.onnx
eco: saved /home/you/.local/share/eco/models/wespeaker_campplus.onnx
eco: Hyprland rules added in /home/you/.config/hypr/bindings.lua; `hyprctl reload` loads them
eco: skill /home/you/.agents/skills/eco/SKILL.md installed
eco: skill /home/you/.claude/skills/eco/SKILL.md installed
```

Run again, it downloads and changes nothing:

```
eco: /home/you/.local/share/eco/models/silero_vad.onnx is up to date
eco: /home/you/.local/share/eco/models/wespeaker_campplus.onnx is up to date
eco: Hyprland rules in /home/you/.config/hypr/bindings.lua are up to date
eco: skill /home/you/.agents/skills/eco/SKILL.md is up to date
eco: skill /home/you/.claude/skills/eco/SKILL.md is up to date
```

`/home/you` throughout this page stands for your home directory.

## 3. Register the models in the window

There is no config to write by hand. Without `~/.config/eco/config.toml` the
daemon starts with an empty one: no model, no audio source, the defaults for
the rest. It starts in the system language: Portuguese, Japanese or English
by `LANGUAGE`, `LC_ALL`, `LC_MESSAGES` or `LANG` (the Windows display language on
Windows), and English for any other. The window's language, the language
sessions are transcribed in and the answer rules follow it; the settings
change each of them. Start eco (step 5) and its window opens; `SUPER+ALT+C` opens the
settings. **Models** registers a transcription model and a chat model, each
with where its key comes from ([how to register models](how-to-register-models.md)),
and **Audio** says who is heard
([how to set up audio sources](how-to-set-up-audio-sources.md)). Each change
is saved on its own — a value in a row, such as a device's **HEARD AS**, saves
at once; an item such as a model opens, **EDIT**, **SAVE** — and the first save
writes the file; from then on the settings window edits it. Every field is in [`design.md` §9](design.md#9-configuration).

Until then, what needs a model says so: a session started transcribes nothing
and says `no transcription model is chosen`, and an answer fails with `no chat
model is chosen`. A model whose key is missing does not stop the daemon
either: everything else works, and what uses that model says why.

## 4. Load the Hyprland rules

The package carries eco's Hyprland rules and keys as one Lua file,
`/usr/share/eco/hypr/eco.lua`. **`eco setup` (step 2) already did this step:**
it added one line to the end of `~/.config/hypr/bindings.lua`
(`$XDG_CONFIG_HOME/hypr/bindings.lua` when that is set) that loads it:

```lua
do local eco = "/usr/share/eco/hypr/eco.lua"; local file = io.open(eco, "r"); if file then file:close(); dofile(eco) end end -- eco setup
```

The line loads the file only while it exists, so Hyprland does not fail on it
once the package is removed. Run `hyprctl reload` to load the rules now.

`eco setup` writes nothing else in your Hyprland config, and it writes this line
only once. Before it changes the file it keeps a copy beside it,
`bindings.lua.bak.<seconds>` (the Unix time, a placeholder here), as Omarchy's
own tools do. A `bindings.lua` it cannot read or write is a warning: the rest of
setup still runs, and it exits with status 1. It leaves the file alone in two
cases, and says so:

- **A line already names an `eco.lua`** — your own `dofile`, say:

  ```
  eco: /home/you/.config/hypr/bindings.lua already loads an eco.lua; left as is
  ```

- **There is no `bindings.lua`**, as in a Hyprland config that is not
  Omarchy's. It does not create one; it prints the line, and you put it in the
  Lua file your config loads:

  ```
  eco: no /home/you/.config/hypr/bindings.lua; add this line to your Hyprland config to load eco's rules: do local eco = …
  ```

**To stop loading the rules,** put `--` in front of the line, which makes it a
comment. A commented line names an `eco.lua`, so `eco setup` leaves it as it is
from then on. Deleting the line works too, until the next `eco setup` adds it
back.

Without the rules Hyprland tiles the window like any other. With it, the window floats,
is pinned to every workspace and keeps the size you give it, and the settings
window opens centred above it. It also binds the global keys:

| keys | what |
|---|---|
| `SUPER+ALT+N` | new session |
| `SUPER+ALT+P` | pause or resume the session shown |
| `SUPER+ALT+H` | the sessions list |
| `SUPER+ALT+C` | open or close the settings |
| `SUPER+ALT+E` | bring eco to the front |
| `SUPER+ALT+A`, `SUPER+ALT+Q` | run the skills named `ask` and `probe` |

Each key runs an `eco window` command, which hands one line to the running
daemon; none of them starts a second daemon. Those that open a view give eco
the keyboard. The last row only works if you have skills by those names —
[how to write skills](how-to-write-skills.md) binds your own.

## 5. Start it

```bash
eco start
```

```
eco: window opened
```

Or open **eco** from the launcher, which runs the same command. `eco start`
starts the user service when nothing answers on the socket, waits for the
daemon, and asks it for a window. With a window already open, it brings that
window forward instead and says `eco: window already open, brought forward`; it
opens another only for a live session no window shows.

![eco's start screen: the masthead "ECO / READY", an input trace swinging across the top, the buttons NEW SESSION, SESSIONS, IMPORT and PEOPLE, and below them "Outside a session the signal is only measured: nothing is transcribed or recorded."](img/01-start.png)

The trace moving is the check that capture works. Outside a session that is all
eco does with the audio: it measures it, and transcribes and keeps nothing.

The rest of the daemon's verbs:

```bash
eco start --headless   # the daemon only, no window: "eco: daemon running"
eco stop               # "eco: daemon stopped"
eco restart            # stop, start, and reopen the window if it was open
eco status
```

```
eco: daemon running (pid 434054, version 0.1.0)
```

`eco status` prints `eco: daemon stopped` and exits 3 when nothing answers, so a
script can test it.

**To have eco in the tray,** open **Settings › General** (`Ctrl+8`) and
press the **OFF** chip of **SHOW ECO IN THE TRAY** to turn it **ON**: it saves at once, and the
icon shows in the bar's tray at once (on
Omarchy it may sit in the drawer behind the `<` arrow). A click opens the
window; its menu opens a window, starts a session, opens the settings or stops
eco. Turn it off, and it goes. It is off until you turn it on, and
it shows only while the daemon runs. The same in `config.toml`:

```toml
[ui]
tray = true
```

**The service is not enabled.** The package carries the unit, and nothing more:
the daemon is not running after a login until `eco start` or the launcher
starts it. A crash restarts it (`Restart=on-failure`).

**To restart the daemon from the window,** open **Settings › General** and
press **RESTART** in the **DAEMON** row of **UPDATES**. It does what `eco
restart` does, and the window reconnects on its own. While a session records
it asks first: if you go on, the session stops recording and comes back
paused, as after any stop of the daemon.

---

## On Windows

eco runs on 64-bit Windows (tested on Windows 11 build 26100) from an MSI, the
Windows Installer package, with the program, its window, the Qt it draws with
and the two models. It needs the Microsoft Visual C++ Redistributable (x64),
which most machines already have. Every
[GitHub release](https://github.com/This-Is-NPC/eco/releases/latest) carries
`eco-<version>-windows-x86_64.msi` (`<version>` is a placeholder for the
release's version, `0.3.0` here) and a `SHA256SUMS` that names it beside the
Arch package ([how it is built](design.md#13-packaging)); `install.ps1`, in
the repository, installs that MSI.

The MSI installs for every user of the machine, in `C:\Program Files\eco`, so
Windows asks for administrator rights once. It puts `eco` on the system
`PATH`, adds **eco** to the Start menu, names eco's notifications and lists
eco in **Settings › Apps**. The models ship inside it: eco needs no download
and no `eco setup` to run.

**eco 0.9 and earlier do not update themselves to the MSI.** Those releases
were a zip installed in `%LOCALAPPDATA%\Programs\eco`, and their updater
installs a zip. Run `install.ps1` once, or the MSI: `install.ps1` stops that
eco and removes its folder, its `PATH` entry, its Start menu shortcut and its
notifications key first. Your config, sessions, people and models stay. Run
the MSI by hand and the old install is still there; remove it with
`$env:ECO_UNINSTALL = "keep"` (below) first, or `install.ps1` after.

1. **Install it,** in PowerShell:

   ```powershell
   irm https://raw.githubusercontent.com/This-Is-NPC/eco/master/install.ps1 | iex
   ```

   It asks GitHub for the latest release, downloads its MSI and
   `SHA256SUMS`, and refuses an MSI that does not match. While the MSI
   downloads, one line shows a bar filling with the percentage, the
   megabytes come and the speed; a connection that drops or sends nothing for
   a minute is tried again, three times in all. Then it stops an eco
   that is running, removes an older per-user install if there is one, and
   runs `msiexec /i` on the MSI: Windows Installer shows its progress and asks
   for administrator rights. The script says so first, because the prompt can
   open behind the terminal, as a flashing shield in the taskbar. A daemon that was running is started again. Every
   line the script writes starts with `eco: `. It ends with a summary: the
   version installed, the program folder (`C:\Program Files\eco`), the
   command and the folder on the system `PATH`, where the settings and the
   sessions are, how to open eco and how to take it off. A failure is said in
   red with what happened and whether anything changed (a declined
   administrator prompt changes nothing; a failed Windows Installer run names
   its log, `%TEMP%\eco-msiexec.log`), followed by the line that tries again. `$env:VERSION = "0.3.0"` before it picks a release
   other than the latest; `$env:DRY_RUN = "1"` downloads and checks the MSI,
   names what it would remove, and stops there. Running it again is the
   upgrade: the MSI replaces the installed eco, also one of the same version,
   and refuses to go back to an older version.

   The programs are not code-signed, so the first time one runs,
   SmartScreen may stop it with "Windows protected your PC": **More info ›
   Run anyway** (read in Microsoft's documentation, not seen).

   **Without the script,** download `eco-<version>-windows-x86_64.msi` and
   `SHA256SUMS` from the release, compare the hashes, and open the MSI:

   ```powershell
   $msi = "eco-0.3.0-windows-x86_64.msi"
   (Get-FileHash $msi -Algorithm SHA256).Hash
   Select-String -SimpleMatch $msi SHA256SUMS
   msiexec /i $msi
   ```

   The two hashes are equal when the MSI is the one the release built. Open
   a new terminal, then `eco --version` says `eco 0.3.0`. To upgrade, run the
   newer MSI. A daemon still running from another copy is restarted by the
   next `eco start` or shortcut, as on Linux.

2. **The models.** They are in the MSI (`share\eco\models` under the install
   folder), and eco reads them from there first. `eco setup` does what step 2
   does on Linux for a build that does not ship them, and for a shipped model
   says so instead of downloading:

   ```
   > eco setup
   eco: Silero VAD ships with eco: C:\Program Files\eco\share\eco\models\silero_vad.onnx
   eco: WeSpeaker CAM++ ships with eco: C:\Program Files\eco\share\eco\models\wespeaker_campplus.onnx
   ```

   (Written from the code; not run on an installed MSI.) The models are
   third-party files under their own licences, which ship as
   `MODELS-LICENSES.txt` beside the program: Silero VAD is MIT, WeSpeaker
   CAM++ is Apache-2.0. `eco setup` also registers the name and icon of eco's
   notifications for your account (below); you never need it after the MSI.

3. **There is no config to write.** As step 3 says for Linux, eco starts
   with an empty config, and the settings write `%APPDATA%\eco\config.toml`
   at the first save.

4. **Give it the keys,** in the Windows Credential Manager (below) or as
   environment variables of your account. `setx` keeps a variable for
   programs started after it, so open a new terminal, and `eco restart` a
   daemon that was running:

   ```powershell
   setx OPENROUTER_API_KEY "<your key>"
   setx DEEPGRAM_API_KEY "<your key>"
   ```

   `<your key>` is a placeholder. Without them eco still starts (seen on
   the machine eco was built on); what needs a model says
   `DEEPGRAM_API_KEY is not set` or `OPENROUTER_API_KEY is not set` (read in
   the code). omapass keeps its secrets in a keyring
   Windows does not have, so a model with `api_key_omapass` says `omapass is
   not available on Windows`.

   **Keys in the Windows Credential Manager.** A model whose key source is
   **CREDENTIAL MANAGER** in Settings › Models names a key, say `deepgram`;
   the daemon reads the generic credential `eco:deepgram` each time it builds
   that model. The window never sees the key. Store it from a terminal, typed
   without showing:

   ```
   > eco key set deepgram
   Key for eco:deepgram (hidden):
   eco: the key is in the Windows Credential Manager as eco:deepgram
   ```

   Piped, it reads the key from stdin instead (`Get-Content key.txt | eco key
   set deepgram`). `eco key remove deepgram` deletes it. Or add it by hand:

   1. Open the Credential Manager: search the Start menu for
      **Credential Manager** (**Gerenciador de Credenciais** in Portuguese),
      or **Control Panel › User Accounts › Credential Manager**, or run
      `control /name Microsoft.CredentialManager`.
   2. **Windows Credentials › Add a generic credential.**
   3. **Internet or network address:** `eco:deepgram`. **User name:**
      anything. **Password:** the key. **OK.**

   Then `eco restart`. A model whose credential is not there says, in the
   window, `Transcription unavailable: deepgram has no key in the Windows
   Credential Manager. Run eco key set deepgram, …`. These outputs are read
   in the code: the machine eco was built on is reached over ssh, and an ssh
   login by key has no Credential Manager, where `eco key` says `the Windows
   Credential Manager cannot be reached from this logon session (Windows
   ERROR_NO_SUCH_LOGON_SESSION); an ssh login by key has none, so run eco
   from the desktop` (seen). A daemon `eco start` started over such a login
   cannot read the keys either; start it from the desktop.

5. **To import audio or video,** put `ffmpeg` and `ffprobe` on your `PATH`
   (the MSI does not carry them; [ffmpeg.org](https://ffmpeg.org/download.html)
   links Windows builds), then `eco restart`: the daemon looks for them on the
   `PATH` it started with. Without them an import says
   `import: ffmpeg is not installed: importing audio or video needs ffmpeg and ffprobe`.

6. **Start it:** open **eco** from the Start menu, or in a terminal:

   ```powershell
   eco start              # "eco: window opened"
   eco start --headless   # "eco: daemon running", without a window
   eco status             # "eco: daemon running (pid 7840, version 0.2.0)"
   eco stop               # "eco: daemon stopped"
   ```

The daemon's verbs are the same as on Linux, with one difference: it runs as
a process of your own in the background rather than a service. These outputs
are from eco's earlier zip package, unpacked and run with data folders of its own on the machine
eco was built on, which is reached over ssh: the models were copied in first,
so `eco setup` found them up to date, and the window opened offscreen
(`QT_QPA_PLATFORM=offscreen`), not on a desktop.

**From a checkout** eco runs as well: its window is then
`target\window\eco-window.exe`, which `mise run window:build` builds, putting
Qt into the checkout the first time;
[how it is built](design.md#working-on-eco) says what Windows needs for that.
Without the window, `eco start` starts the daemon, then fails to open the
window with `Error: eco window failed:` and the path it looked for.

**To install from a checkout,** in place of step 1, run
`mise run install:windows`. It builds the MSI into `dist` (`mise run
package`, which needs the pinned WiX and downloads the models once), writes a
`SHA256SUMS` beside it, and runs `install.ps1` on that folder instead of a
release: the same steps as step 1. On Linux the task refuses and names `mise
run install`.

**To take it off again,** run the installer with `ECO_UNINSTALL`: `keep`
keeps your sessions, config and models, `everything` deletes them too.

```powershell
$env:ECO_UNINSTALL = "keep"; irm https://raw.githubusercontent.com/This-Is-NPC/eco/master/install.ps1 | iex
Remove-Item env:ECO_UNINSTALL
```

It stops the daemon and its window, removes an older per-user install if
there is one, and runs `msiexec /x` on the installed eco, which removes the
program, its `PATH` entry, the Start menu entry and the notifications key,
saying `eco: removed` and the path for each, and `eco: kept` for the data
folders it leaves. `everything` also removes `%APPDATA%\eco` and
`%LOCALAPPDATA%\eco`: the config, the sessions, the people, the downloaded
models, the token and the log. With `$env:DRY_RUN = "1"` it only names them.
You can also remove **eco** in **Settings › Apps**; that keeps all your data.

From a checkout, `mise run uninstall:windows` runs the same removal and
asks first:

```
1) uninstall eco, keep sessions, config and models
2) uninstall and delete everything
Choose 1 or 2:
```

`2` asks again, `Type delete to delete your sessions, config and models:`,
and only the word `delete` goes on. Any other answer says `nothing removed`
and removes nothing. `mise run uninstall:windows --keep-data` and `mise run
uninstall:windows --delete-everything` answer without asking.

**The window may still open behind the others.** Windows brings a window to
the front only for a program that has the foreground or was given it, and the
daemon runs in the background. When a shortcut or `eco window` asks for eco,
its window joins the input of the program in front for a moment and takes the
foreground from it; if Windows refuses even so, the window stays behind and
flashes in the taskbar: click it there.

**Global shortcuts.** There is no Hyprland on Windows: the daemon registers
the keys itself, those of Linux with the Windows key for SUPER. None is a key
Windows keeps for itself: `Win+Alt` with a digit opens a taskbar jump list, and
the Game Bar holds `Win+Alt+G`, `M`, `R` and `T`.

| Keys | What they do |
|---|---|
| `Win+Alt+N` | open the dialog that starts a session |
| `Win+Alt+H` | the sessions |
| `Win+Alt+C` | the settings |
| `Win+Alt+P` | pause or resume the session shown |
| `Win+Alt+E` | bring eco to the front |
| `Win+Alt+A`, `Win+Alt+Q` | run the skills `ask` and `probe` |

Other keys go under `[shortcuts]` in `%APPDATA%\eco\config.toml` (create the
file if it is missing; the settings window has no place for them), each the
keys and the `eco window` command they run; the table replaces the defaults,
and an empty one binds none ([the configuration](design.md#9-configuration)):

```toml
[shortcuts]
"Win+Alt+A" = "action ask"
"Ctrl+Alt+M" = "sessions"
```

The keys are read when the daemon starts: `eco restart` after a change. Keys
another program holds are not taken from it, and that is no error: the
shortcuts are optional, and the others work. The daemon's log says it, and
`eco doctor` passes the check naming the keys left off:

```
eco: Win+Alt+A is taken by another program
```

A daemon started where there is no desktop, over ssh say, has every key
refused and says so for each. On a desktop, `Win+Alt+C`, `N`, `H` and `E`
were pressed and worked; `Win+Alt+A` and `Q` were chosen after `Win+Alt+1` and
`2` were refused there, and were not pressed on a desktop yet.

**Hiding from screen sharing** (**Settings › General › HIDE FROM SCREEN
SHARING**) leaves eco's windows out of every capture, your own screenshots and
recordings too; Microsoft documents this for Windows 10 2004 and later
(read, not seen).

**The tray icon** is turned on the same way, in **Settings › General ›
SHOW ECO IN THE TRAY**; it shows in the notification area, perhaps among the
hidden icons behind its `^` arrow, from which Windows lets you drag it onto
the taskbar. Its click and menu do what they do on Linux.

**The daemon's output** goes to `%LOCALAPPDATA%\eco\daemon.log`, emptied each
time `eco start` starts it. A daemon that will not start says why there, and
`eco start`, after waiting 10 seconds for it, repeats that line:

```
Error: the daemon was started, but did not open the socket; it said:
  eco: missing C:\Users\you\AppData\Local\eco\models\silero_vad.onnx; run `eco setup`
```

Only a line the daemon starts with `eco: ` or `Error: ` is repeated; the rest
of the log stays in the file.

(`C:\Users\you` stands for your profile directory.)

**eco does not start at login** on Windows: it runs once you open it, from
the Start menu entry below or with `eco start`, and keeps running when the
terminal that ran `eco start` is closed. It does not start from an
Administrator terminal, so that the Start menu entry and the windows can
reach the daemon: `eco start` and `eco restart` there fail with `eco runs
without administrator rights, so every window can reach it; run this from a
terminal that is not elevated`. If a daemon an earlier eco started elevated
is running, run `eco stop` from the elevated terminal, then start eco again.
An earlier eco wrote a
login script, `eco.cmd`, into your Startup folder; `eco setup` and the installer remove it and
say so:

```
eco: removed C:\Users\you\AppData\Roaming\Microsoft\Windows\Start Menu\Programs\Startup\eco.cmd; eco no longer starts at login
```

With no script there, they say nothing about it. A script it cannot remove is
a warning, and `eco setup` exits with status 1.

**The Start menu.** The MSI adds the shortcut **eco** to the Start menu of
every user, with eco's icon. It runs `eco-launch.exe`, which does what `eco
start` does but is a program with no console, so no console window opens while
it waits for the daemon and its window. A failure, such as running from an
elevated session, shows as a notification. `eco setup` does not touch it, so
a build from a checkout does not take it over.

**The notifications' name.** The MSI registers eco's app id, the name and
icon Windows shows on eco's notifications, for every user under
`HKLM\Software\Classes\AppUserModelId\This-Is-NPC.eco`: `DisplayName` `eco`
and `IconUri` the `share\eco\eco.ico` of the install. `eco setup` registers it
in your account's registry (`HKCU`, the same path) too, which a build from a
checkout needs:

```
eco: notifications registered as eco's (This-Is-NPC.eco)
```

Run again, it says nothing; from an eco that has moved, `notifications
updated as eco's (This-Is-NPC.eco)`. The daemon registers it again before
each notification, so they say "eco" even if setup never ran. A key it cannot
write is a warning, `eco setup` exits with status 1, and the notifications say
"Windows PowerShell".

**To take a build from a checkout off Windows,** stop it and delete the
checkout and the notifications' registry key `eco setup` wrote for it:

```powershell
eco stop
Remove-Item -Recurse "HKCU:\Software\Classes\AppUserModelId\This-Is-NPC.eco"
```

Your config is in `%APPDATA%\eco` and the
sessions, people, models and log in `%LOCALAPPDATA%\eco`; delete those
folders to remove them too. A key you gave with `setx` stays until you remove
it, one command each:

```powershell
[Environment]::SetEnvironmentVariable("OPENROUTER_API_KEY", $null, "User")
```

---

## Update

eco says when a newer release is out, and installs it when you ask, on Linux
and on Windows. The outputs below are real: they came from the checkout's eco
in a home of its own under `/tmp`, asking the real GitHub, with stand-ins for
`pkexec` and `systemd-run` that only noted what they were asked. `0.4.0` was
the latest release then; `0.5.0` below is an example.

**How it knows.** The daemon asks GitHub for the tag of the latest release:
when it starts, if the last check is older than 24 hours, then every 24 hours
while it runs. It is one GET with `User-Agent: eco/<version>` (`<version>` is
a placeholder for the installed one); nothing about you is sent, and there is
no telemetry. What it found is kept in `~/.local/state/eco/update.json`
(`$XDG_STATE_HOME/eco/` when that is set; `%LOCALAPPDATA%\eco\update.json` on
Windows), so a restart does not ask again:

```json
{"checked_at":1791997033,"latest":"0.4.0","notes":"https://github.com/This-Is-NPC/eco/releases/tag/v0.4.0"}
```

When the latest release is newer than the one running, the main window shows a
small notice, `ECO 0.5.0 AVAILABLE`, in the masthead, except on a
session and on its cost. A press opens the release notes in your browser; its
× hides it until the window opens again. With **SYSTEM NOTIFICATIONS** on
(**Settings › General › INTERFACE**) and no eco window active, the system also
notifies you, once per version.

**To never ask on its own,** turn **CHECK FOR UPDATES** off in **Settings ›
General › UPDATES**, or in `config.toml`:

```toml
[update]
check = false
```

The daemon then never asks GitHub by itself, and the window shows no notice.
What you start still asks, once: **CHECK NOW**, `eco update`, and `eco
version` when no daemon runs.

### From the window

1. Open **Settings › General** (`Ctrl+8`). The **UPDATES** group has three rows:
   **CHECK FOR UPDATES**, an ON/OFF chip; **VERSION**, with **CHECK NOW** and
   `0.4.0 · up to date · checked 2 h ago` (or `0.4.0 · not checked`,
   `0.4.0 · could not check`),
   or **UPDATE** and `0.4.0 · 0.5.0 available` when a newer one is out; and
   **DAEMON**, with **RESTART** and `running`.
2. Press **UPDATE**. The row and a toast say each step: `downloading eco
   0.5.0`, `checking the checksum of eco 0.5.0`, `installing eco 0.5.0`,
   `restarting eco`, `eco 0.5.0 is running` — or `update failed:` and the
   reason, in an error toast. A release GitHub does not have said:
   `update failed: could not download v0.4.1/SHA256SUMS: 404 Not Found`.
3. **On Linux,** at installing, `pkexec` asks for your password in a window of
   its own, and installs the package with `pacman -U`, as `install.sh` does.
   (Read in the code: the runs behind this page used a stand-in `pkexec`.)
4. The daemon restarts on the new version, as `eco restart` does, and the
   window reopens if it was open. On Linux the restart runs as `systemd-run
   --user --collect --quiet /usr/bin/eco restart`, so stopping the service does
   not stop the restart with it.

**RESTART** in the **DAEMON** row restarts the daemon the same way. While a
session records it asks first, **RESTART ECO**: "A session is recording.
Restarting eco now stops its recording." **CANCEL** leaves it; **RESTART**
restarts it, and the session comes back paused, as after any stop of the
daemon.

### From a terminal

`eco update --check` only asks:

```console
$ eco update --check
{"ok":true,"data":{"current":"0.4.0","latest":"0.4.0","newer":false,"notes":"https://github.com/This-Is-NPC/eco/releases/tag/v0.4.0"}}
```

`eco update` installs the latest release when it is newer, and does nothing
otherwise; `eco update --version <version>` installs that release, an older
one too:

```console
$ eco update
{"ok":true,"data":{"current":"0.4.0","version":"0.4.0","installed":false,"restart":"none"}}
$ eco update --version 0.4.0
eco: downloading 0.4.0
eco: checking 0.4.0 against its SHA256SUMS
eco: installing 0.4.0
{"ok":true,"data":{"current":"0.4.0","version":"0.4.0","installed":true,"restart":"none"}}
```

The steps go to stderr, the JSON to stdout. Who installs depends on the
daemon:

- **A daemon runs:** it installs the release, the same code as **UPDATE**, and
  restarts. `restart` is `"now"`, or `"when_session_ends"` while a session
  records. Until that restart, another `eco update` is refused with
  `update.busy`.
- **No daemon runs,** or one older than 0.5.0 that does not know updates:
  `eco update` installs it itself and `restart` is `"none"`. An older daemon
  goes on running the old binary until the next `eco start` restarts it, as
  above.

On Linux it runs `pkexec pacman -U --noconfirm` on the package. Without
`pkexec`, in a terminal, it runs `sudo pacman -U` and `sudo` asks there.
Without either it installs nothing and downloads nothing:

```json
{"ok":false,"code":"update.failed","message":"pkexec is not installed, and there is no terminal for sudo to ask for the password; install polkit, or run eco update in a terminal"}
```

A password prompt closed or refused:

```json
{"ok":false,"code":"update.failed","message":"the password was not given; nothing was installed"}
```

The codes: `update.unavailable` (GitHub could not be asked, or has no such
release: `GitHub answered 404 for releases/tags/v9.9.9`), `update.failed` (the
download, the checksum or the install failed; the message says which) and
`update.busy`.

### What it checks

Every release carries a `SHA256SUMS`. eco downloads it, picks the asset for
this system from it — the Arch package `eco-<version>-1-x86_64.pkg.tar.zst`,
or the Windows installer `eco-<version>-windows-x86_64.msi` — downloads that into a
folder of its own under the temporary directory and checks its sha256. When it
does not match, the file is deleted, nothing is installed, and the update
fails with `eco-<version>-1-x86_64.pkg.tar.zst does not match the release's
SHA256SUMS; nothing was installed` (tested against a local server; a real
release has never mismatched). The folder goes once the install ends.

### A recording is never cut

The install happens while a session records; the restart waits. The row and
a toast say `eco 0.5.0 installed · restarts when the session ends`, and
the daemon restarts once the last live or paused session ends. Until then it
keeps running the old version.

### On Windows

eco installs the release's `eco-<version>-windows-x86_64.msi` per machine in
`C:\Program Files\eco`. Windows Installer cannot replace an eco that runs, so
the update downloads and checks the file while eco runs, and the rest happens
once no session is live: eco copies itself to the temporary directory, stops
the daemon and its windows, and runs `msiexec /i <file> /qb /norestart` from
the copy. Windows Installer shows its progress and asks for the administrator
rights itself (the UAC prompt); eco never runs elevated. When it ends, eco
starts again from `C:\Program Files\eco\bin\eco.exe`, with its window if it
was open.

If the install does not finish (you closed the installer or refused the
prompt, another installation was running, or the installer failed), the old
eco starts again and a notification says `eco was not updated:` and why. The
downloaded file is deleted either way.

`eco update` with no daemon running downloads and checks the file, then starts
the same installer in its own process and returns: its progress shows in
Windows Installer's window, and a failure shows as a notification, not in the
terminal. eco is not started afterwards, as no daemon ran.

### Which version, and is everything in place

`eco version` prints the installed version, the running daemon's (`null` when
none runs), the latest release, and the channel, always `release`:

```console
$ eco version
{"ok":true,"data":{"installed":"0.4.0","daemon":"0.4.0","latest":"0.4.0","channel":"release"}}
```

With a daemon, `latest` is what that daemon last found, `null` before its
first check; with none, `eco version` asks GitHub itself.

`eco doctor` checks the installed files, the VAD and speaker models, that the
config loads, that each model's key source resolves (by name; it never reads
out a key), that the daemon runs and is this eco, and the global shortcuts
(that the Hyprland config loads eco's rules on Linux; the keys the daemon
registered on Windows). It prints one JSON object, a list of `{check, ok,
detail, fix}`, and exits 1 when any check fails. From the checkout's eco, in
the home under `/tmp` with no speaker model and no Hyprland config
(`/home/you/eco` is a placeholder for the checkout):

```console
$ eco doctor
{"ok":false,"data":[{"check":"installed","ok":false,"detail":"/home/you/eco/target/debug/eco runs without lib/eco/eco-window, share/eco/overlay/shell.qml beside it","fix":"install eco again (docs/how-to-install-and-remove.md)"},{"check":"models","ok":false,"detail":"missing: WeSpeaker CAM++ (/tmp/eco-r7/.local/share/eco/models/wespeaker_campplus.onnx)","fix":"run eco setup"},{"check":"config","ok":true,"detail":"/tmp/eco-r7/.config/eco/config.toml loads","fix":""},{"check":"key:whisper-lan","ok":true,"detail":"needs no key","fix":""},{"check":"key:deepgram","ok":true,"detail":"DEEPGRAM_API_KEY is set","fix":""},{"check":"key:gemini-flash","ok":true,"detail":"OPENROUTER_API_KEY is set","fix":""},{"check":"key:claude-haiku","ok":true,"detail":"OPENROUTER_API_KEY is set","fix":""},{"check":"daemon","ok":true,"detail":"running, version 0.4.0","fix":""},{"check":"shortcuts","ok":false,"detail":"cannot read /tmp/eco-r7/.config/hypr/bindings.lua: No such file or directory (os error 2)","fix":"run eco setup"}]}
```

---

## What the install puts on the machine

The package owns these; `pacman -Ql eco` lists every file and where it is:

- the `eco` binary;
- its window, the QML the daemon launches;
- the Hyprland rules and keys, `eco.lua`;
- the launcher and its icon;
- the bash, zsh and fish completions of `eco`, which complete through the
  `usage` command the package depends on;
- the licence;
- the user service, which runs `eco daemon --headless`.

`eco setup` (step 2) writes these in your home, and they are not the package's:

| what | where |
|---|---|
| the VAD and speaker models | `~/.local/share/eco/models/` |
| the agent skill | `~/.agents/skills/eco/SKILL.md` and `~/.claude/skills/eco/SKILL.md` |
| the line loading the Hyprland rules | the end of `~/.config/hypr/bindings.lua` |

And what eco writes as it is used, which is yours and not the install's:

| what | where |
|---|---|
| the config | `~/.config/eco/config.toml` |
| the sessions, one append-only log each | `~/.local/share/eco/sessions/` |
| the people you named, and their voices | `~/.local/share/eco/people/` |

**No audio is ever written.** A session's log is text: lines, notes, answers.
Voices are kept as numbers that describe them, not as sound.
[`design.md` §8](design.md#8-state-on-disk) lists every file.

The skill is how an agent learns the `eco` command line. `eco setup` refreshes
the copies it owns and never overwrites a file at those paths that it did not
write.

---

## Taking it off

eco takes itself off. Removing it keeps your data by default: the config, the
sessions, the people and the models stay.

```bash
eco uninstall               # keeps your data
eco uninstall --everything  # also deletes the config, sessions, people and models
```

It asks no question: the command is the question. A running daemon takes the
removal and refuses it while a session records (`uninstall.recording`); end the
session first. With no daemon, `eco uninstall` hands the removal off itself.
It prints `{"uninstalled":true,"data":"kept"}` (`"deleted"` with
`--everything`) once the removal has started.

**On Linux,** a process of its own (`eco apply-uninstall`, in a transient
`systemd-run --user` unit, so stopping the daemon does not stop it) stops the
daemon, then runs `pkexec pacman -Rns --noconfirm eco`, which shows the
desktop's password prompt; without `pkexec`, in a terminal, `sudo pacman -Rns`.
That removes everything the package put on the machine: the binary, the window,
the Hyprland rules, the launcher, the icon, the completions, the licence and the
user service. With `--everything` it then deletes `~/.config/eco`,
`~/.local/share/eco` and `~/.local/state/eco`. A removal that fails, or a
password prompt that is dismissed, deletes nothing. A notification says how it
ended.

**On Windows,** a copy of `eco.exe` in the temporary directory stops the daemon
and runs `msiexec /x` for every eco Windows Installer lists, which shows its
progress and asks for the elevation. With `--everything` it then deletes
`%APPDATA%\eco` and `%LOCALAPPDATA%\eco`. A toast says how it ended: removed, `eco
was not removed: the removal was cancelled; nothing changed` when the
installer's window is closed or the elevation refused, or the reason it
failed. eco does not start again. The copy stays in the temporary directory,
which Windows clears. A key in the Credential Manager stays: remove it with
`eco key remove <name>` first. `install.ps1` with `ECO_UNINSTALL` (above) does
the same from outside eco.

**From the window,** **Settings › General** has an **UNINSTALL** group with an
**UNINSTALL** button. It opens a dialog that says what goes: the program, the
window, the shortcuts and the service. **ALSO DELETE MY DATA** is off:
sessions, people, config and models stay. Turn it on and **UNINSTALL** deletes
them too. **CANCEL** closes the dialog and changes nothing. Confirming sends the
same command as `eco uninstall`; the window closes as eco stops. While a
session records, the answer is an error toast: `A session is recording; end it
before removing eco`.

**The line `eco setup` added to `~/.config/hypr/bindings.lua` stays,** and
does nothing: it loads the rules only while the file exists. To remove it too,
delete the line that ends in `-- eco setup`.

**Kept on purpose, unless you ask for everything:** the config, the sessions,
the people and the models. Removing the program is not a reason to lose a year
of meetings. **Never removed by eco:** the agent skill, which lives in other
programs' folders:

```bash
rm -rf ~/.agents/skills/eco ~/.claude/skills/eco
```

Keys you put in `~/.config/uwsm/env` stay there until you delete those lines.

Without eco running, `sudo pacman -Rns eco` removes the package alone, and
`rm -rf ~/.config/eco ~/.local/share/eco ~/.local/state/eco` the data.
(Read in the code; the Linux and Windows removals have not been run in the
real packages.)

---

## What can go wrong

**Something is missing, and you do not know what.** `eco doctor` runs every
check and names the fix for each that fails ([Update](#which-version-and-is-everything-in-place)).

**The window does not open after an install on a system not updated lately.**
A release is built against the Qt Arch ships that day, and needs that Qt or a
newer one. Up to 0.5.0 the package did not say so, and `journalctl --user -u
eco` shows, each time the daemon tries to open a window:

```
/usr/lib/eco/eco-window: /usr/lib/libQt6Core.so.6: version `Qt_6.12' not found (required by /usr/lib/eco/eco-window)
```

Update the system (`sudo pacman -Syu`, or `omarchy-update`), then `eco
restart`. Later packages depend on that Qt release, so pacman brings it along
when it installs eco.

**The models are missing.** The daemon will not start without them, and says
which:

```
eco: missing /home/you/.local/share/eco/models/silero_vad.onnx; run `eco setup`
```

`eco start` waits 10 seconds for the daemon, then repeats the line it left in
the journal of `eco.service`:

```
Error: the daemon was started, but did not open the socket; it said:
  eco: missing /home/you/.local/share/eco/models/silero_vad.onnx; run `eco setup`
```

**The config does not validate.** The daemon names the rule it broke, one line,
and does not start:

```
eco: a provider's key comes from one of api_key_env, api_key_omapass or api_key_credential
```

**The window says OFFLINE.** Its start screen adds `Daemon disconnected · eco
start`: nothing answers on `$XDG_RUNTIME_DIR/eco.sock`. `eco status`, then
`eco start`; the window reconnects on its own. Service failures
are in `journalctl --user -u eco.service`.

![eco's start screen with no daemon: the masthead "ECO / OFFLINE", the buttons dimmed, under them, in amber, "Daemon disconnected · eco start", and in the bottom-right corner an error toast saying the daemon is disconnected.](img/02-disconnected.png)

---

## Next

- [How to register models](how-to-register-models.md) — the keys, and which
  model transcribes and which answers.
- [How to set up audio sources](how-to-set-up-audio-sources.md) — who eco hears,
  and which voice is yours.
- [The command line](cli.md) — every command and flag.
