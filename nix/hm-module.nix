self:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  inherit (lib)
    mkEnableOption
    mkIf
    mkOption
    types
    ;

  cfg = config.programs.cosmic-ext-quake-terminal;

  appId = "com.github.m0rf30.CosmicExtQuakeTerminal";
  configDir = "cosmic/${appId}/v1";

  exe = lib.getExe cfg.package;

  # The daemon only looks at the basename of the command to pick the right
  # window-class flags, so a full store path ("/nix/store/...-ghostty/bin/ghostty")
  # works and doesn't depend on $PATH.
  terminalCommand =
    if cfg.terminal.package != null then lib.getExe cfg.terminal.package else cfg.terminal.command;

  # JSON strings and lists of strings are valid RON, which is what cosmic-config
  # stores (one file per field).
  ron = builtins.toJSON;

  toggleEntry =
    "  (modifiers: [${lib.concatStringsSep ", " cfg.keybind.modifiers}], key: ${ron cfg.keybind.key}): "
    + "Spawn(${ron "${exe} toggle"}),";

  shortcutsFile = lib.concatStringsSep "\n" (
    [ "{" ]
    ++ lib.optional (cfg.keybind != null) toggleEntry
    ++ lib.optional (cfg.extraShortcuts != "") (lib.removeSuffix "\n" cfg.extraShortcuts)
    ++ [
      "}"
      ""
    ]
  );

  manageShortcuts = cfg.keybind != null || cfg.extraShortcuts != "";
in
{
  options.programs.cosmic-ext-quake-terminal = {
    enable = mkEnableOption "the COSMIC quake-style dropdown terminal";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = lib.literalExpression "cosmic-ext-quake-terminal.packages.\${system}.default";
      description = "The cosmic-ext-quake-terminal package to use.";
    };

    terminal = {
      package = mkOption {
        type = types.nullOr types.package;
        default = null;
        example = lib.literalExpression "pkgs.ghostty";
        description = ''
          Terminal emulator to spawn. When set, the package is added to
          `home.packages` and its main program (as a full store path) is used
          as the command, overriding `terminal.command`.
        '';
      };

      command = mkOption {
        type = types.str;
        default = "cosmic-term";
        example = "ghostty";
        description = ''
          Terminal command, resolved through `$PATH`. Ignored when
          `terminal.package` is set. Terminals the daemon knows about:
          cosmic-term, ghostty, alacritty, kitty, foot, wezterm.
        '';
      };

      args = mkOption {
        type = types.listOf types.str;
        default = [ ];
        example = [
          "--some-flag"
          "value"
        ];
        description = "Extra arguments passed to the terminal on spawn.";
      };
    };

    keybind = mkOption {
      type = types.nullOr (
        types.submodule {
          options = {
            modifiers = mkOption {
              type = types.listOf (
                types.enum [
                  "Super"
                  "Ctrl"
                  "Alt"
                  "Shift"
                ]
              );
              default = [ ];
              example = [ "Super" ];
              description = "Modifier keys.";
            };
            key = mkOption {
              type = types.str;
              example = "F12";
              description = "XKB keysym name, e.g. `F12`, `grave`, `space`.";
            };
          };
        }
      );
      default = null;
      example = {
        modifiers = [ "Super" ];
        key = "grave";
      };
      description = ''
        Shortcut that runs `cosmic-ext-quake-terminal toggle`. When set (or
        when `extraShortcuts` is non-empty) this module owns COSMIC's
        `com.system76.CosmicSettings.Shortcuts/v1/custom` file, so any custom
        shortcuts you also want must be declared via `extraShortcuts`.
        The file becomes read-only, so COSMIC Settings can no longer edit
        custom shortcuts.
      '';
    };

    extraShortcuts = mkOption {
      type = types.lines;
      default = "";
      example = ''
        (modifiers: [Super], key: "e"): Spawn("cosmic-files"),
      '';
      description = ''
        Extra entries (raw RON map entries, each ending in a comma) added to the
        custom shortcuts file next to `keybind`.
      '';
    };
  };

  config = mkIf cfg.enable {
    home.packages = [ cfg.package ] ++ lib.optional (cfg.terminal.package != null) cfg.terminal.package;

    xdg.configFile = {
      "${configDir}/terminal_command".text = ron terminalCommand;
      "${configDir}/terminal_args".text = ron cfg.terminal.args;
    }
    // lib.optionalAttrs manageShortcuts {
      "cosmic/com.system76.CosmicSettings.Shortcuts/v1/custom".text = shortcutsFile;
    };
  };
}
