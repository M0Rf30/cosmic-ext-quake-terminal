{
  lib,
  rustPlatform,
  pkg-config,
  libcosmicAppHook,
  libxkbcommon,
  wayland,
  libinput,
  udev,
  dbus,
  fontconfig,
  freetype,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
  appId = "com.github.m0rf30.CosmicExtQuakeTerminal";
in
rustPlatform.buildRustPackage {
  pname = cargoToml.package.name;
  inherit (cargoToml.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../i18n.toml
      ../i18n
      ../src
      ../data
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    # libcosmic / cosmic-protocols are git dependencies. This avoids having to
    # maintain an outputHashes table every time the revs in Cargo.toml change.
    allowBuiltinFetchGit = true;
  };

  nativeBuildInputs = [
    pkg-config
    libcosmicAppHook
  ];

  buildInputs = [
    libxkbcommon
    wayland
    libinput
    udev
    dbus
    fontconfig
    freetype
  ];

  # Needs a live COSMIC session / Wayland compositor, so skip tests.
  doCheck = false;

  # cargo's install hook already installs the binary; the justfile only adds
  # the desktop entry and icon on top of that.
  postInstall = ''
    install -Dm0644 data/${appId}.desktop -t $out/share/applications
    install -Dm0644 data/icons/${appId}.svg \
      -t $out/share/icons/hicolor/scalable/apps
  '';

  meta = {
    description = cargoToml.package.description;
    homepage = cargoToml.package.repository;
    license = lib.licenses.gpl3Only;
    mainProgram = "cosmic-ext-quake-terminal";
    platforms = lib.platforms.linux;
  };
}
