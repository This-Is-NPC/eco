# How do I change the appearance?

Open Settings and choose Appearance (Ctrl+9).

Choose a preset:

- **System** follows the Omarchy theme on Linux. On Windows it follows the
  system light/dark app preference. Changes apply while eco is open.
- **Dark** always uses eco's dark palette.
- **Light** always uses eco's light palette.
- **Custom** lets you choose background, text, accent, secondary, borders,
  success, warning, error and alternative colors.

Under Custom, click a swatch to open the color dialog, or enter a six-digit
hexadecimal color such as `#39ff14`. Press Enter or leave the field to save
it. Invalid values stay in the field with an error and are not saved. Each
accepted change saves immediately and reaches all open eco windows.

The first Custom selection copies the current theme. Switching to another
preset keeps the custom colors for the next time. **Reset colors** restores
the built-in dark colors while keeping Custom selected.

A note reports low text contrast. eco adjusts the displayed text to black
or white when its contrast against the background would fall below 4.5:1.
Accent and state colors also use the theme's readability adjustment.
The selected color values stay saved as entered.

The preferences live under `[ui.appearance]` in `config.toml`:

```toml
[ui.appearance]
preset = "custom"

[ui.appearance.colors]
background = "#121212"
foreground = "#e5e2e1"
accent = "#39ff14"
```

Omitted custom colors use the dark palette. An existing config without
appearance settings uses System.

See [installation](how-to-install-and-remove.md) for the config location,
and [the design](design.md#121-visual-language-and-the-kit) for the theme architecture.
