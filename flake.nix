{
  description = "Quake-style dropdown terminal for COSMIC Desktop";

  nixConfig = {
    extra-substituters = [ "https://jasper-cosmic-ext-quake-terminal.cachix.org" ];
    extra-trusted-public-keys = [
      "jasper-cosmic-ext-quake-terminal.cachix.org-1:lq7WSGMrMXH2VQc8tF94A23NABfRFeTXbz4vX5G+/2E="
    ];
  };

  inputs.nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        cosmic-ext-quake-terminal = pkgs.callPackage ./nix/package.nix { };
        default = cosmic-ext-quake-terminal;
      });

      overlays.default = final: _prev: {
        cosmic-ext-quake-terminal = final.callPackage ./nix/package.nix { };
      };

      # Home Manager module: programs.cosmic-ext-quake-terminal.*
      homeManagerModules.default = import ./nix/hm-module.nix self;
      homeModules.default = self.homeManagerModules.default;

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          packages = with pkgs; [
            cargo
            rustc
            clippy
            rustfmt
            rust-analyzer
            just
          ];
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
            with pkgs;
            [
              wayland
              libxkbcommon
              vulkan-loader
              libGL
            ]
          );
        };
      });

      formatter = forAllSystems (pkgs: pkgs.nixfmt);
    };
}
