# Omarea: Thurea, made for Omarchy

Omarea (Omarchy + Thurea) is Thurea's app on [Omarchy](https://omarchy.org). It isn't a separate
program bolted onto the desktop: it fits into the places Omarchy already has, looks like the rest
of Omarchy, and follows every theme switch. Thurea's protection itself doesn't depend on Omarea,
so taking Omarea out never weakens anything.

Installing Thurea's package (`sudo pacman -U thurea-linux.pkg.tar.zst`) fits Omarea into the
desktop of every Omarchy user on the computer, with nothing to set up. Users added later, and a
reinstalled Omarchy, get it on Thurea's next check (every 5 minutes).

## Where it shows up

| | Omarchy 4 (omarchy-shell) | Omarchy 3 (Waybar, Walker, mako) |
|---|---|---|
| **Bar** | The Omarea widget, first on the right: Thurea's shield, with a dot in the theme's accent colour while a family request waits. Click for the panel, middle-click to set up a phone | An Omarea module first in Waybar's right side, with the same clicks, and right-click for Family |
| **Panel** | The shield draws itself; protection status, the verse of the day, the family's requests and each child's screen time today, Set up a phone, Family, Re-apply, Settings. Keys: **F** family, **D** downloads, **P** phone, **R** re-apply, **S** settings, **Esc** close | Thurea's window (thurea-tray, also in Waybar's tray) |
| **Menu** | An **Omarea** submenu in the Omarchy menu (SUPER + SPACE): Open Omarea, Family, Video downloads, Set up a phone, Keep my phone, Re-apply protection, Notifications (✓ when on), Check Omarea. All searchable: try "parental" or "screen time" | — (the launcher entries below) |
| **Launcher** | Omarea, Omarea Family and Omarea: Set up a phone in the app launcher | The same in Walker |
| **Keys** | **SUPER + CTRL + U** Omarea · **SUPER + SHIFT + CTRL + U** Family, listed with their names in SUPER + K | The same |
| **Notifications** | Omarchy's own, with buttons: a child asks for an app or a site (*Open Family*), protection stopped for more than a moment (*Re-apply now*) and is back, a child has 10 minutes of screen time left | The same through mako, without buttons |
| **Theme** | Colours, fonts, spacing and motion are the shell's (`qs.Commons`), so the widget changes with the theme. The family centre (`thurea control-center`) reads the current theme's `colors.toml` and recolours itself within seconds of a theme switch, with secondary text and button labels kept readable (4.5:1 and 3:1 contrast) on every theme | The same for the family centre; the Waybar module uses Waybar's theme colours |

Family notifications go only to parents (the computer's user holds a parent key of the family),
and an old request isn't announced again after a restart.

## Commands

```sh
omarea                  # open Omarea (the panel)
omarea family           # the family control centre
omarea downloads        # video downloads
omarea phone            # set up a new or factory-reset Android phone
omarea keep-phone       # set up a phone that's already in use (nothing erased)
omarea reapply          # re-apply Thurea's protection now
omarea status           # one line; --json for Thurea's report; --waybar for Waybar
omarea notifications on | off | toggle
omarea doctor           # checks every piece and says how to fix what's missing
omarea install          # fit Omarea in again (after taking it out of the bar, for example)
omarea uninstall        # take Omarea out of this desktop
```

On Omarchy 4 the panel also answers the shell's IPC:
`omarchy-shell thurea.omarea toggle | open | close | setupPhone | keepPhone | settings | family |
downloads | reapply | refresh | status`.

## What it changes in your files

Omarea follows Omarchy's conventions for third-party additions and keeps out of Omarchy's own
files (everything under `$OMARCHY_PATH` stays untouched, so Omarchy's updates never conflict):

| File | What | Omarchy |
|---|---|---|
| `~/.config/omarchy/plugins/thurea.omarea` | a link to the plugin Thurea's package keeps up to date (`/usr/share/thurea/omarchy-plugin`) | 4 |
| `~/.config/omarchy/shell.json` | Omarea's place in the bar, through the running shell's own `enablePlugin` when it's running | 4 |
| `~/.config/omarchy/extensions/omarchy-menu.jsonc` | the Omarea submenu | 4 |
| `~/.config/hypr/bindings.lua` (4) or `bindings.conf` (3) | the two keybindings | both |
| `~/.config/waybar/config.jsonc`, `style.css` | the Omarea module and its style | 3 |
| `~/.config/thurea/` | Omarea's own marks (installed, placed in the bar once, uninstalled) and `omarea.conf` | both |

Every addition to a file of yours sits between `Omarea (Thurea) begin` and `Omarea (Thurea) end`
lines, and `omarea uninstall` takes out exactly those lines: the file is left byte for byte as it
was (the tests check this). If you already use SUPER + CTRL + U, Omarea leaves your binding alone,
skips its own and says so.

Taking Omarea out of the bar (Setup › Plugins, or the bar's own settings) is respected: Thurea's
updates won't put it back; `omarea install` will. `omarea uninstall` takes all of Omarea out, and
that is respected too until you run `omarea install`.

## Coming from the Thurea bar plugin

The bar plugin was called `thurea.omarchy` before. The first `omarea install` (which Thurea's
update runs by itself) moves its place in the bar over to `thurea.omarea`, settings included, and
removes the old link, so nothing jumps around.

## Without Thurea's package

`omarchy plugin add https://github.com/ShaddickDev/thurea.git --enable` installs the widget alone
from Omarchy's plugin manager (the repository's root `manifest.json` points at the plugin). Until
Thurea itself is installed, it says how to install it.

## For developers

* The plugin: `linux/omarchy-plugin/omarea/` (`Panel.qml` the bar widget and panel,
  `Service.qml` the notification clock, `ThureaMark.qml` the logo). It holds no logic of its own:
  every answer comes from the `thurea` command.
* Everything else: `linux/files/omarea` (bash; one file for both Omarchy versions), started for
  every user by `linux/files/thurea-omarchy`.
* Tests: `linux/tests/omarea-test.sh` (install, upgrade from the old plugin, uninstall, Omarchy 3,
  keys already taken, the notifications, doctor) and `cargo test -p thurea-agent` (the theme
  reader). On an Omarchy machine, `omarchy plugin validate linux/omarchy-plugin/omarea` and
  `qs log -p "$OMARCHY_PATH/shell" --tail 100` after `omarea install`.
