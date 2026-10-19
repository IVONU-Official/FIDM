# FIDM

FIDM (Free Internet Download Manager) is an open-source desktop download manager written in Rust. It aims to bring IDM-style segmented downloads and queue management to Linux and, over time, other desktop platforms.

> **Development status:** early prototype. The Linux GUI and basic HTTP download flow are under active development. See the roadmap before relying on FIDM for important downloads.

## Current features

- Native desktop interface built with `eframe` and `egui`.
- SQLite-backed download list and status history.
- HTTP and HTTPS downloads, with up to eight byte-range connections when the server supports ranges.
- In-session pause and resume, retry handling, progress, speed, and ETA display.
- Extension-based categories and a configurable destination folder per added download.
- `wgpu` rendering backend for the desktop UI.

Downloads paused during a running session can continue in that session. Partial byte positions are not yet saved across application restarts, so an unfinished file starts again from the beginning after restarting FIDM. Downloads from servers that do not support range requests use a single connection.

## Linux setup

Install Rust with [rustup](https://rustup.rs/) and the native development packages. On Debian or Ubuntu:

```sh
sudo apt install build-essential pkg-config libx11-dev libxcursor-dev libxrandr-dev libxi-dev libxkbcommon-dev libwayland-dev libvulkan-dev
```

From the project directory, build and launch:

```sh
cargo run --release
```

The first build downloads Rust dependencies. FIDM stores its database at `$XDG_DATA_HOME/fidm/fidm.sqlite3`, or `$HOME/.local/share/fidm/fidm.sqlite3` when `XDG_DATA_HOME` is unset. The default download directory is `$HOME/Downloads`.

## Project structure

```text
src/
  main.rs             Application startup and Tokio runtime
  model.rs            Download model, status, and category helpers
  db/mod.rs           SQLite storage
  downloader/mod.rs   HTTP range transfer engine
  ui.rs               eframe/egui interface
```

## Roadmap

- [x] Linux desktop application scaffold
- [x] SQLite persistence for download records
- [x] Parallel HTTP range transfers and in-session pause/resume
- [ ] Persist completed byte ranges and resume after restart
- [ ] Add a destination directory picker and URL metadata preview
- [ ] Queue editing, scheduling, and queue completion actions
- [ ] Global and per-download bandwidth limits
- [ ] Configurable folders for each file category
- [ ] Mirror management, broader retry policies, and download validation
- [ ] Expand automated coverage and platform builds

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) for local setup, code style, and pull request guidance. The [Code of Conduct](CODE_OF_CONDUCT.md) applies to project spaces. Report vulnerabilities using the process in [SECURITY.md](SECURITY.md).

## License

FIDM is distributed under the MIT License. See [LICENSE](LICENSE).
