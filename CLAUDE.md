## Layout

One module per feature. Each one keeps its platform-free logic at the top, where `cargo test`
runs it on any OS. Its Windows code sits in a `#[cfg(windows)] mod win` in the same file.

```
src/main.rs      entry; Windows-only modules are cfg-gated
src/settings.rs  settings file, styles, sizes, accents, widgets, quick-control buttons, widget rows
src/dates.rs     civil dates, ISO times, weekday/month names, clock formats
src/http.rs      URL encoding; curl.exe with headers and forms on stdin
src/weather.rs   Open-Meteo geocoding and forecast
src/calendar.rs  iCal parsing and repeats; fetch and local time on Windows
src/google.rs    Google Calendar: OAuth pieces, events; sign-in, DPAPI token on Windows
src/limits.rs    Claude and Codex plan limits
src/apps.rs      app identity and dock order; pins, windows, icons, activation on Windows
src/dock.rs      window/bar geometry and the magnification curve
src/glass.rs     glass parameters per style and the displacement map
src/system.rs    CPU, memory, battery, keyboard layout, keys, clock, startup key
src/taskbar.rs   hiding Explorer's taskbar and aligning Start
src/net.rs       network speed from the adapters' byte counters
src/controls.rs  media session, Wi-Fi/Bluetooth radios, VPN, volume
src/feeds.rs     background fetching of weather, calendar, limits and releases
src/update.rs    update check: the latest GitHub release against this build's version
src/tray.rs      hidden-icons host: receives tray icons, forwards everything to Explorer
src/gfx.rs       D3D11/DComp/D2D setup, glass pipeline, drawing primitives, image cache
src/ui.rs        themes, metrics, layout and every bar tile
src/panel.rs     calendar, weather, AI, quick controls, about and hidden-icons popovers
src/prefs.rs     the settings window and its editing state
src/app.rs       windows and input, one action dispatcher, one settings pipeline
build.rs         embeds icons/serenebar.ico into the exe (Windows targets)
icons/           app icon: serenebar.svg is the source; PNGs and the .ico are rendered from it
prek.toml        git hooks: fmt and clippy before commit, commit message rules
committed.toml   conventional-commit rules for the commit-msg hook
```
