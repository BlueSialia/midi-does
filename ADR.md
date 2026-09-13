# Architectural Decision Records

All Architectural Decisions and their rationale will be documented in this file.

The format is based on the [Y-Statement](https://adr.github.io/adr-templates/#y-statement).

---

## Architecture & Project Structure

### Single Crate Structure

In the context of organizing config parsing, MIDI handling, PipeWire communication, UI rendering, and action execution, facing the choice between a single crate and a workspace, we decided for a single Rust crate with modules to achieve simplicity and avoid build complexity for a small codebase, accepting that all code compiles as one unit and module boundaries are enforced only by `pub` visibility.

---

## GUI & UI

### Iced for GUI

In the context of building a Linux desktop application with two-panel layout, forms, picklists, and modal dialogs, facing the need for a GUI framework, we decided for Iced (v0.14) to achieve a pure-Rust stack with no foreign language bindings or JavaScript toolchain, accepting lock-in to the `wgpu` rendering backend and the need to bridge background threads via `mpsc` channels instead of Iced's async channels.

### Two-Panel Layout and Modal Editing

In the context of showing configured MIDI devices and editing their control mappings, facing the need for a UI layout and an editing model, we decided for a two-panel layout with the device list on the left (1:3 ratio) and a modal overlay for icon editing that writes directly to in-memory config and saves on close, to achieve a familiar list-detail pattern and a simple, natural editing flow, accepting fixed fill portions with no resizable split, potential scrolling on very narrow windows, and the loss of unsaved changes if the app is restarted mid-edit (there is no cancel/undo path).

### X11 Backend Preference for Hide-to-Tray

In the context of hiding the window to the system tray when the user closes it, facing native Wayland's lack of a window-hide/unmap API, we decided for preferring X11/XWayland by unsetting `WAYLAND_DISPLAY` and `WAYLAND_SOCKET` whenever `DISPLAY` is set to achieve reliable hide-to-tray and taskbar removal, accepting that native Wayland sessions are never used and the environment mutation is process-wide.

---

## MIDI

### midir for MIDI I/O

In the context of discovering and reading from ALSA MIDI ports and writing MIDI output for LED/display feedback, facing the need for a Rust MIDI library, we decided for midir (v0.11) to achieve a simple, well-established API with transparent ALSA backend handling and output support, accepting that `MidiInput::connect()` consumes `self` (requiring a fresh `MidiInput` per connection) and that ports are identified by name.

### Relative Encoder Decoding

In the context of handling endless rotary encoders that send relative values instead of absolute positions, facing different encoding schemes across controllers, we decided for a selectable decoding scheme (Sign-Magnitude, Two's Complement, Binary Offset) multiplied by a configurable step to achieve compatibility with the X-TOUCH MINI and other controllers without hardcoding, accepting per-action encoder configuration and the default step of 0.01 (1% volume).

### Manual Device Discovery (No Auto-Discovery)

In the context of MIDI devices connecting and disconnecting dynamically, facing the risk of adding virtual ports and system ports to the config, we decided for manual device discovery requiring the user to click "+ Add Device" and pick from a filtered list to achieve explicit user control over which devices are managed, accepting that a first-time user sees an empty device list and must take explicit action.

### MIDI Message Parsing and Output

In the context of MCU-compatible controllers sending motorized fader positions via pitch bend and exchanging display feedback via SysEx, and controllers needing to receive LEDs, rings, scribble strips, and arbitrary messages, facing incomplete MIDI event coverage and the need for MIDI output, we decided for adding `MidiEvent::PitchBend` (14-bit scaled to 0–127) and `MidiEvent::SysEx` (raw bytes, logged but not dispatched) plus a single `MidiOutputHandle` per device shared by `MidiSendNote`/`MidiSendCc` actions and feedback entries, to achieve one output connection covering explicit sends and display feedback, accepting that SysEx events have no dispatch path, the unscaled 14-bit value is only visible in the monitor, and LED-specific actions were removed in favor of feedback-driven display (the blink state machine was dropped).

### Name-Based MIDI Port Identity and Hotplug Reconnection

In the context of attributing MIDI events to a configured device and reconnecting after hotplug, facing instability from midir's fresh ALSA client per `connect()` and from ALSA client IDs (`client_id:port_id`) that the kernel reassigns on every registration, we decided for a stable port identity derived from the `client_name:port_name` prefix of the midir port name, used as the config key and stored in `port_name`, to achieve consistent device resolution across replugs/reboots, duplicate-free reconnects via `HashMap<port_name, MidiInputConnection>`, and automatic input/output handle recreation on hotplug, accepting that two identical controllers sharing the same `client_name:port_name` are indistinguishable by name alone and that ports are polled every 3 seconds.

---

## PipeWire & Audio

### Native PipeWire Operations and Stream Metadata

In the context of listing PipeWire objects, reading their state, executing audio operations (volume, mute, routing), and labeling streams readably, facing pipewire-rs's complex and poorly documented SPA parameter API, we decided for using the `pipewire` crate for both live monitoring and native volume/mute/routing operations, and for extracting `application.name`, `application.process.binary`, `node.nick`, or `media.name` from node properties to build `app_name (node_name)` stream labels, to achieve event-driven state tracking and operations without spawning a process per object or action, accepting a 500 ms "fight window" so relative volume changes have a known starting point and stream labels that depend on the properties each stream publishes.

### Stable PipeWire Object References and Unknown Volume Semantics

In the context of configured PipeWire mappings breaking on every reboot because numeric ids change, and relative encoder changes jumping to 100% for objects without parser-reported volume, facing non-persistent references and fabricated baseline values, we decided for persisting stable node names instead of numeric ids and making volume an `Option<f64>`, with node listeners providing live volume/mute for streams and filters to achieve reboot-proof mappings and correct relative volume changes, accepting that legacy numeric ids are not auto-migrated and filters/streams carry `None` volume until a Props event or probe provides the real value.

### Device Route Writes for Hardware Endpoints

In the context of volume and mute actions targeting physical endpoints (ALSA sinks and sources), facing the session manager and desktop settings UI reading the endpoint volume/mute from the Device's active Route while midi-does wrote only the node's Props (so hardware changes were audible but invisible in the UI), we decided for also writing the target Device's active Route whenever a volume or mute change targets a node backed by a device route, to achieve a single source of truth between midi-does and the session manager, accepting Device tracking (device id plus `card.profile.device` read from node `info` events, since it is absent from registry global props) and redundant node-Props plus route writes that keep audio and UI in sync.

---

## Configuration & Data Model

### TOML Configuration at XDG_CONFIG_HOME

In the context of persisting device mappings and control assignments, facing the need for a configuration format and location, we decided for TOML stored at `$XDG_CONFIG_HOME/midi-does/config.toml` to achieve human-readable, editable configuration in the standard user config location, accepting no auto-generation of default configs, no file watching, and the requirement to restart the app to pick up external edits.

### Hardware/Software Split

In the context of representing real-world MIDI controllers and giving users maximum flexibility, facing a configuration model that conflated physical device properties with behavioral mappings, we decided for splitting device configuration into hardware and software to achieve a clean separation of concerns, accepting a breaking config format change.

Hardware models the physical device once and is shared across all layers: the grid dimensions, each control's type (button, knob, fader, encoder), its grid position and size, and its MIDI inputs and outputs. Software captures everything the user wants to do, per layer and per control: a display label, one or more actions per input, a source per output, and a source driving the control's faceplate visual. Layers multiply the mapping surface without duplicating the hardware definition.

Consequences folded in from earlier decisions: icon placement, type, spans, inputs, and outputs live at the device level rather than per layer; the icon label lives in per-layer software so the same control can show a different name per layer; and the active layer is tracked per device (runtime-only, initialized to the first layer, not persisted) so MIDI dispatch and bank operations stay correct across multiple controllers.

---

## Threading & Events

### Channel-Based Threading with 33 ms Polling

In the context of communicating MIDI and PipeWire events from background threads to the Iced GUI event loop, facing the choice between sync and async channel approaches, we decided for `std::sync::mpsc` channels drained every 33 ms via `iced::time::every` to achieve a zero-dependency solution, accepting ~0.1% CPU overhead from polling the empty channels.

### Off-Thread Feedback Evaluation

In the context of feedback sources spawning subprocesses (`Source::Custom`) and freezing the GUI when evaluated synchronously, and feedback being re-evaluated on every PipeWire snapshot (every 50 ms) then applied to hardware outputs, facing the need for non-blocking evaluation without an unbounded backlog or redundant hardware writes, we decided for a dedicated worker thread fed by a latest-snapshot-wins bus (one pending slot each for feedback and visuals) with results delivered back on the 33 ms tick, and per-control value-change detection comparing against the last-sent result instead of a time-based rate limit, to achieve prompt feedback latency with immediate updates on change and zero redundant SysEx/CC writes, accepting a worker round-trip plus command runtime latency, a 1 s timeout on `Source::Custom` shell commands, and a per-control `HashMap` of last-sent results.

The worker drains both pending slots together each cycle rather than prioritizing feedback, because a feedback slot continuously refilled every 50 ms by the PipeWire publish loop would otherwise starve the visuals slot while a slow `Source::Custom` feedback command holds the worker thread, leaving the faceplate in its neutral state indefinitely.

---

## Actions & Controls

### Shell Command Execution with Variable Substitution

In the context of running user-defined shell commands triggered by MIDI events with access to the MIDI value, facing the need to execute arbitrary commands, we decided for `$SHELL -c` with `$RAW_VALUE`, `$NORM_VALUE`, and `$BANK_OFFSET` variable substitution to achieve command syntax matching the user's own terminal environment, accepting fire-and-forget execution with no output capture (for actions), no timeout or lifecycle management, and the risk of shell injection from user-controlled data. Built-in audio operations are performed through the native PipeWire API, never through a shell.

### Routing Sources into Sinks

In the context of the Route Stream action and the RouteActive feedback source, facing the need to wire a hardware input (e.g. a microphone) into a virtual sink for mixing, we decided for allowing sources on the source side of the link while keeping sinks as the only destination, to achieve virtual-microphone setups without new node types, accepting that routing remains restricted to output-to-input links (a hardware mic cannot be a destination, since it exposes only output ports).

---

## Feedback & Visuals

### Unified Source Abstraction

In the context of feedback outputs, visual sources, and conditional logic all needing to produce values from PipeWire state, MIDI input, or user-defined expressions, facing the need for a single, composable value-producing abstraction (earlier attempts used recursive rule trees that had type-mismatch bugs between rule results and output kinds, and `And`/`Or`/`Not` combinators with no working UI editor), we decided for one `Source` enum used everywhere — feedback entries, per-icon `visual_source`, and `If` condition operands and branches — with each feedback entry mapping directly to an output and the logical combinators removed, to achieve one source language with one editor widget, recursive composability, and no mismatch bugs or dead-end conditions created through the UI, accepting a breaking config format change and that all sources evaluate to a single text result that the consumer interprets by kind (boolean for LEDs, numeric for rings, text for scribble strips).

Source types:

- **Leaf sources** querying PipeWire or layer state: `Volume`, `Muted`, `RouteActive`, `LayerActive`, `Bank`.
- **Custom** — a user-defined shell command whose stdout becomes the source value, enabling anything the built-in sources cannot express.
- **Direct** — a literal fixed value typed by the user: a boolean (`true`/`false`), a number (e.g. `0.75`), or a string (e.g. `"MUTE"`). Always evaluates to itself.
- **HardwareInput** — the most recent raw MIDI value on any input of the same control, normalized to 0.0–1.0, so a control's visual or feedback can mirror its own input.
- **If** — a recursive conditional source with three parts: `condition` (two sources compared with numeric/string operators), `then` (a source evaluated when the comparison is true), and `else` (a source evaluated when false). Because `If` is itself a source, it can be nested inside another `If`'s then/else or condition operands, forming arbitrarily deep logic trees.
- **RingRange** — linearly maps a source value into an output range (`min`–`max`) and normalizes it to 0.0–1.0 for LED ring outputs. The source value is not clamped: values below 0.0 or above 1.0 extrapolate linearly below `min` or above `max`.
- **Blink** — alternates between two sources at a configurable interval.

Faceplate icons render exclusively from their own `visual_source`, never from the raw MIDI value, so a control's visual reflects configured audio/layer state and shares the same source language as its feedback; icons without a visual source render in their neutral state.

### Scribble Strip and LED Ring Feedback via SysEx

In the context of MCU controllers having scribble strip LCDs and V-Pot LED rings, facing the need for feedback output, we decided for `Scribble` and `LedRing` output kinds using MCU SysEx (`F0 00 00 66 14 …`), with `LedRingCc` via standard CC as a fallback, to achieve 7-character × 2 line ASCII display and LED ring positioning, accepting that Pan/Fan/Spread modes are not implemented and non-ASCII characters are replaced with spaces.

---

## CI, Releases & Packaging

### Automated CI, Releases, and Packaging

In the context of shipping a desktop application to Linux users and keeping code quality consistent, facing a manual release and distribution process, we decided for a Conventional-Commits-driven automation stack (git hooks, GitHub Actions CI, cargo-deny, git-cliff, and cargo-deb/cargo-generate-rpm packaging) producing per-release `.deb`, `.rpm`, and Nix flake recipe artifacts, to achieve reproducible checks and easy installation across Debian/Ubuntu, Fedora/openSUSE, and NixOS, accepting more moving parts in CI, a commit-message convention, and build-time maintenance of packaging metadata in `Cargo.toml` and the flake.
