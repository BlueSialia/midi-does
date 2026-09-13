<p align="center">
  <img src="assets/midi-does.svg" alt="MIDI Does logo" width="160">
</p>

# MIDI Does

[![Hippocratic License HL3-BOD-CL-ECO-FFD-LAW-MEDIA-MIL-MY-SV-TAL-USTA-XUAR](https://img.shields.io/static/v1?label=Hippocratic%20License&message=HL3-BOD-CL-ECO-FFD-LAW-MEDIA-MIL-MY-SV-TAL-USTA-XUAR&labelColor=5e2751&color=bc8c3d)](https://firstdonoharm.dev/version/3/0/bod-cl-eco-ffd-law-media-mil-my-sv-tal-usta-xuar.html)

---

Map any MIDI controller to PipeWire audio controls and shell commands. Use knobs, faders, and buttons on your MIDI hardware to manage system audio — volume, mute, routing, default devices — or run arbitrary commands with MIDI value substitution.

## Feature Coverage

<details>
<summary>Feature checklist</summary>

Test functions are tagged with `#feature <ID>`.

> `[x]` tested &nbsp; `[ ]` supported, no test yet &nbsp; `[-]` not supported

### MIDI

- [x] `MIDI-CC` — CC message input
- [x] `MIDI-NOTE` — Note On message input (Note Off ignored — actions fire once)
- [x] `MIDI-PB` — Pitch bend input
- [x] `MIDI-SX` — SysEx message input
- [ ] `MIDI-LEARN` — MIDI learn mode (auto-detect CC/Note/PitchBend)
- [ ] `MIDI-MON` — MIDI event monitor panel
- [ ] `MIDI-HOT` — Hotplug auto-reconnect for known devices
- [x] `MIDI-PORTS` — MIDI port discovery and enumeration
- [ ] `MIDI-OUT` — MIDI output (Note, CC, SysEx)
- [-] `MIDI-AT` — Channel / polyphonic aftertouch
- [-] `MIDI-THRU` — MIDI pass-through
- [-] `MIDI-CLOCK` — MIDI Clock sync (send or receive)
- [-] `MIDI-VCURVE` — Velocity curve adjustment

### Actions

- [x] `ACT-VOL` — Volume control: sink, source, stream (absolute & relative)
- [x] `ACT-MUTE` — Mute toggle: sink, source
- [x] `ACT-ROUTE` — Stream routing
- [-] `ACT-DEF` — Set default sink / source
- [x] `ACT-CMD` — Run shell command with $RAW_VALUE, $NORM_VALUE, $BANK_OFFSET
- [-] `ACT-LED` — LED on / off / toggle / blink
- [x] `ACT-MIDI` — MIDI send action (Note, CC)
- [x] `ACT-LAYER` — Software layer selection
- [x] `ACT-BANK` — Bank offset / increment / decrement
- [x] `ACT-MULTI` — Multiple actions per control
- [x] `ACT-ENC` — Encoder modes: Sign-Magnitude, Two\'s Complement, Binary Offset
- [-] `ACT-TOGGLE` — Toggle vs momentary button mode
- [-] `ACT-COND` — Conditional actions (e.g. if volume > X)
- [-] `ACT-MACRO` — Macro chains with delays

### MCU Protocol

- [ ] `MCU-FADER` — MCU fader input (pitch bend with bank offset)
- [x] `MCU-SCRIB` — Scribble strip output (7-char × 2 line SysEx)
- [x] `MCU-RING` — LED ring output via MCU SysEx (Single mode; Pan/Fan/Spread not implemented)
- [ ] `MCU-BANK` — Fader bank offset (+8 / -8 / set)
- [-] `MCU-VPOT` — V-Pot assignment modes
- [-] `MCU-JOG` — Jog wheel support
- [-] `MCU-TC` — Timecode display

### Faceplate

- [x] `FACE-GRID` — Configurable icon grid (rows × columns)
- [x] `FACE-ICON` — Icon types: Button, Knob, Fader, Encoder
- [ ] `FACE-SPAN` — Icon span (multi-cell, e.g. 1×4 fader)
- [ ] `FACE-EDIT` — Icon editor modal with inline control editing
- [ ] `FACE-MULTI` — Multiple controls per icon
- [x] `FACE-VIS` — Visual state on icons
- [-] `FACE-COLOR` — Custom icon / grid colors
- [-] `FACE-REORDER` — Drag-and-drop icon reordering
- [-] `FACE-TEMPLATE` — Icon templates for known controllers

### Reactive Feedback

- [ ] `FB-MUTE` — Mute state → button LED mirroring (via feedback source)
- [x] `FB-RING` — Volume level → LED ring segments via feedback source (MCU SysEx `led_ring` or standard CC `led_ring_cc` outputs)
- [ ] `FB-SCRIB` — PipeWire object name → scribble strip auto-population (via feedback source)
- [-] `FB-RATE` — Feedback rate limiting / debouncing (80 ms)
- [ ] `FB-CUSTOM` — User-defined feedback sources (`Source::Custom` commands)

### PipeWire

- [x] `PW-OBJ` — Object listing (sinks, sources, streams)
- [x] `PW-VOL` — Volume state monitoring
- [ ] `PW-MUTE` — Mute state monitoring
- [ ] `PW-CONN` — PipeWire connection monitoring
- [ ] `PW-META` — Stream metadata enrichment 
- [-] `PW-METER` — Audio level meters in the UI
- [-] `PW-MATRIX` — Audio routing matrix view
- [-] `PW-EQ` — EQ / audio effect controls

### Configuration

- [x] `CFG-DEV` — Device configuration persistence (TOML)
- [ ] `CFG-CTRL` — Control mapping persistence (TOML)
- [x] `CFG-LOAD` — Config load with graceful defaults
- [x] `CFG-SAVE` — Config save with directory creation
- [-] `CFG-IMPORT` — Import / export individual device configs
- [-] `CFG-PRESET` — Presets for known controller models

### UI / System

- [x] `UI-TRAY` — System tray integration (icon click/`Show` restores, `Quit` exits; close button hides to tray)
- [ ] `UI-THEME` — System theme detection (dark / light)
- [ ] `UI-KEYS` — Keyboard shortcuts (Escape, Ctrl+S)
- [ ] `UI-MODAL` — Modal dialogs (add device, icon editor)
- [ ] `UI-STATUS` — Status bar (status message, active layer)
- [-] `UI-HEADLESS` — CLI / headless mode (no GUI)
- [-] `UI-MULTIWIN` — Multiple device windows
- [x] `UI-MINIMIZE` — Minimize to tray on close

</details>

## Getting Started

These instructions will get you a copy of the project up and running on your local machine for development and testing purposes. See [Install](#install) for notes on how to install the project on a live system.

### Prerequisites

- **Rust** (stable, via [rustup](https://rustup.rs) or [mise](https://mise.jdx.dev))
- **System libraries**: `pipewire`, `alsa-lib`, `udev`, `vulkan-loader`, `libxkbcommon`, `libx11`, `libxcb`, `libxcursor`, `libxrandr`, `libxi`, `libxext`, `libxinerama`, `libxxf86vm`, `wayland`, `dbus`, `libclang`, plus `pkg-config`
- A running **PipeWire** session
- A **MIDI controller** connected to your system (ALSA MIDI)

The exact package names vary by distribution; most systems ship these split into development packages (`-dev` on Debian/Ubuntu, `-devel` on Fedora/openSUSE). On NixOS none of this needs to be installed by hand. The development shell provides it (see [Nix](#nix)).

### Setup

Clone the repository and build:

```bash
git clone git@github.com:BlueSialia/midi-does.git
cd midi-does
cargo build --release
```

The binary will be at `target/release/midi-does`.

Run `./setup-hooks.sh` once to enable the Conventional Commits commit-msg hook (see [Automations](#automations)).

#### Nix

The repository ships a `flake.nix`, providing the Rust toolchain and the system libraries listed above. On NixOS (or anywhere with Nix and flakes enabled) this is the most automatic way to get started: with [direnv](https://direnv.net) installed, `direnv allow` is enough; otherwise enter the shell explicitly.

```bash
nix develop
```

Then run the usual `cargo` commands inside the shell, or prefix them with
`nix develop --command`, e.g. `nix develop --command cargo build --release`.

### Running the Client

```bash
cargo run
```

Or run the built binary directly:

```bash
./target/release/midi-does
```

## Running the Tests

### Unit Tests

```bash
cargo test
```

Tests are located in each source file (inline `#[cfg(test)]` modules) and tagged with `#feature <ID>` comments matching the feature checklist above.

### Integration Tests

Integration tests live in the `tests/` directory and exercise end-to-end config roundtrips through the filesystem.

```bash
cargo test --test config_integration
```

### Linting

```bash
cargo clippy -- -D warnings
```

### Coding Style

Standard `rustfmt`:

```bash
cargo fmt --check
```

## Install

Each GitHub release includes a `.deb`, a `.rpm`, and the raw binary, along with a Nix recipe for Nix users.

If you want to build and install from source then:

Build the release binary and place it anywhere in `$PATH`:

```bash
cargo build --release
install -m 755 target/release/midi-does ~/.local/bin/
```

To install the desktop entry and icons (for launching from your application menu):

```bash
install -Dm 644 data/midi-does.desktop ~/.local/share/applications/midi-does.desktop
for size in 16 22 24 32 48 64 128 256 512; do
    install -Dm 644 "assets/midi-does-$size.png" \
        "$HOME/.local/share/icons/hicolor/${size}x${size}/apps/midi-does.png"
done
install -Dm 644 assets/midi-does.svg \
    ~/.local/share/icons/hicolor/scalable/apps/midi-does.svg
```

The desktop entry launches `midi-does` from `$PATH` and uses the `midi-does` icon name, so both the binary and the icons must be installed as shown above.

The app reads its configuration from `$XDG_CONFIG_HOME/midi-does/config.toml` (typically `~/.config/midi-does/config.toml`). No other files are created at runtime.

`configs/x-touch mini.toml` is a complete example device configuration (an X-TOUCH MINI mapping). Copy it to your config path as a starting point, e.g. `cp "configs/x-touch mini.toml" ~/.config/midi-does/config.toml`.

## Built With

| Dependency | Purpose |
|---|---|
| [iced](https://iced.rs) | GUI framework |
| [midir](https://crates.io/crates/midir) | MIDI input discovery and message parsing |
| [pipewire](https://crates.io/crates/pipewire) (v0.10) | PipeWire bindings (object discovery, volume/mute, routing) |
| [ksni](https://crates.io/crates/ksni) | KDE StatusNotifierItem system tray |
| [toml](https://crates.io/crates/toml) + [serde](https://serde.rs) | Configuration file parsing |
| [dirs](https://crates.io/crates/dirs) | XDG base directory paths |
| [dark-light](https://crates.io/crates/dark-light) | System theme detection (dark / light) |
| [thiserror](https://crates.io/crates/thiserror) | Error type derivation |
| [log](https://crates.io/crates/log) + [env_logger](https://crates.io/crates/env_logger) | Logging |
| MCU Protocol | Mackie Control Universal protocol (SysEx, pitch bend, scribble strips, LED rings) |

## Automations

CI, release packaging, and git hooks are documented in [AUTOMATIONS.md](AUTOMATIONS.md).

- Run `./setup-hooks.sh` once to enable the Conventional Commits commit-msg hook locally.
- CI runs formatting, Clippy, tests, docs, unused-dependency checks (`cargo machete`), and a `cargo-deny` security/license audit.
- Every merge to `main` opens an automated release PR; merging it (squash) tags a release and attaches `.deb` and `.rpm` packages plus a Nix recipe.

## Contributing

Please read [CONTRIBUTING.md](https://gist.github.com/PurpleBooth/b24679402957c63ec426) for details on our code of conduct, and the process for submitting pull requests to us.

## Versioning

We use [SemVer](http://semver.org/) for versioning. For the versions available, see the [tags on this repository](https://github.com/BlueSialia/midi-does/tags).

## Authors

* **Jorge Domínguez** - *Initial work* - [BlueSialia](https://github.com/BlueSialia)

See also the list of [contributors](https://github.com/BlueSialia/midi-does/contributors) who participated in this project.

## License

This project is licensed under the Hippocratic License 3.0 - see the [LICENSE.md](LICENSE.md) file for details

## Acknowledgments

- The [midir](https://crates.io/crates/midir) and [pipewire-rs](https://crates.io/crates/pipewire) maintainers
- The [Iced](https://iced.rs) project for a pure-Rust GUI toolkit
- PipeWire and WirePlumber for the modern Linux audio stack
