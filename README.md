# COSMIC Quake Terminal

A quake-style dropdown terminal for [COSMIC Desktop](https://github.com/pop-os/cosmic-epoch).

Runs as a background daemon and toggles a terminal emulator's visibility via a keyboard shortcut, similar to Guake, Yakuake, or the Quake console.

## How it works

- Runs as a background daemon with no visible window
- On first toggle, spawns your configured terminal emulator
- Subsequent toggles hide (minimize) or show (activate + focus) the terminal
- Uses COSMIC's Wayland toplevel management protocol (`zcosmic_toplevel_manager_v1`) for window control
- D-Bus activation handles IPC between the CLI toggle command and the running daemon

## Installation

### Building from source

**Dependencies:**

- Rust toolchain (stable)
- [just](https://github.com/casey/just) command runner
- Wayland development libraries
- A running COSMIC Desktop session

```sh
git clone https://github.com/m0rf30/cosmic-ext-quake-terminal.git
cd cosmic-ext-quake-terminal
just build-release
sudo just install
```

### Nix / NixOS

The repo is a flake that provides a package, an overlay, a dev shell and a Home Manager module.

```sh
nix build            # build
nix run . -- toggle  # run the toggle command
nix develop          # dev shell with cargo, clippy, just, ...
```

Declarative setup (terminal and keybind included) with Home Manager:

```nix
# flake.nix
inputs.cosmic-ext-quake-terminal.url = "github:m0rf30/cosmic-ext-quake-terminal";
# (no nixpkgs.follows, so the prebuilt binary from the cache can be used)

# home-manager module list
imports = [ inputs.cosmic-ext-quake-terminal.homeManagerModules.default ];

programs.cosmic-ext-quake-terminal = {
  enable = true;
  terminal.package = pkgs.ghostty;   # or: terminal.command = "kitty";
  # terminal.args = [ "--some-flag" ];
  keybind = {
    modifiers = [ "Super" ];
    key = "grave";
  };
};
```

Notes:

- **Binary cache:** CI pushes builds to Cachix. To skip compiling, add the cache to
  your own Nix config (flake `nixConfig` of the *top-level* flake, or `nix.settings`):
  ```nix
  nix.settings = {
    substituters = [ "https://jasper-cosmic-ext-quake-terminal.cachix.org" ];
    trusted-public-keys = [
      "jasper-cosmic-ext-quake-terminal.cachix.org-1:lq7WSGMrMXH2VQc8tF94A23NABfRFeTXbz4vX5G+/2E="
    ];
  };
  ```
  The cache only hits if the package is built with the same nixpkgs as this flake's
  `flake.lock`, so leave out `inputs.nixpkgs.follows` for this input if you want
  prebuilt binaries.

- `keybind` / `extraShortcuts` make the module own COSMIC's custom shortcuts file
  (`com.system76.CosmicSettings.Shortcuts/v1/custom`). Declare any other custom
  shortcuts through `extraShortcuts` (raw RON entries).
- Values managed by Nix are read-only symlinks, so the in-app settings window and
  COSMIC Settings can't change them. Delete any pre-existing hand-written files at
  those paths before the first activation.

### Uninstall

```sh
sudo just uninstall
```

## Configuration

Configuration is stored at `~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v1/` using COSMIC's config system.

### Setting the terminal emulator

Write the terminal binary name to the config file:

```sh
# Use ghostty
mkdir -p ~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v1
echo '"ghostty"' > ~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v1/terminal_command

# Or use cosmic-term (default)
echo '"cosmic-term"' > ~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v1/terminal_command
```

Changes are picked up automatically without restarting the daemon.

### Supported terminals

| Terminal | Notes |
|----------|-------|
| `cosmic-term` | Default. Uses `--class` for window identification. |
| `ghostty` | Spawns with `--gtk-single-instance=false` to avoid joining existing instances. Tracked via its default `com.mitchellh.ghostty` app ID. |
| `alacritty` | Uses `--class` for window identification. |
| `kitty` | Uses `--class` for window identification. |
| `foot` | Uses `--app-id` for window identification. |
| `wezterm` | Uses `--class` for window identification. |
| Other | Falls back to `--class`. May work if the terminal supports it. |

### Additional terminal arguments

```sh
echo '["--some-flag", "value"]' > ~/.config/cosmic/com.github.m0rf30.CosmicExtQuakeTerminal/v1/terminal_args
```

## Keyboard shortcut

### Via COSMIC Settings

Add a custom shortcut in **Settings > Keyboard > Shortcuts > Custom**:
- Key: `F12` (or your preferred key)
- Command: `cosmic-ext-quake-terminal toggle`

### Via config file

Add to `~/.config/cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom`:

```ron
(
    modifiers: [],
    key: "F12",
): Spawn("cosmic-ext-quake-terminal toggle"),
```

### Known issue: shortcut stops working after Alt-Tab

There is a [known bug](https://github.com/pop-os/cosmic-epoch/issues/2481) in the COSMIC compositor where custom `Spawn` shortcuts may stop firing after using Alt-Tab. The [GlobalShortcuts portal](https://github.com/pop-os/xdg-desktop-portal-cosmic/issues/4) is not yet implemented in COSMIC, so the app cannot register its own global shortcut.

**Workaround:** Use an evdev-based hotkey daemon that bypasses the compositor's shortcut system:

#### swhkd (Simple Wayland HotKey Daemon)

```sh
# Install (AUR)
paru -S swhkd

# Create config
mkdir -p ~/.config/swhkd
cat > ~/.config/swhkd/swhkdrc << 'EOF'
F12
  cosmic-ext-quake-terminal toggle
EOF

# Run (swhks handles the unprivileged side, swhkd needs root for evdev)
swhks &
pkexec swhkd
```

#### Manual toggle

If the shortcut stops responding, you can always toggle from any terminal:

```sh
cosmic-ext-quake-terminal toggle
```

## Usage

The daemon starts automatically via D-Bus activation when you first run the toggle command. You can also start it manually:

```sh
# Start the daemon
cosmic-ext-quake-terminal &

# Toggle the terminal
cosmic-ext-quake-terminal toggle
```

### Debug logging

```sh
RUST_LOG=cosmic_ext_quake_terminal=debug cosmic-ext-quake-terminal
```

## License

GPL-3.0-only
