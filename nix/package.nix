{
  lib,
  rustPlatform,
  pkg-config,
  python3,
  vulkan-loader,
  alsa-lib,
  alsa-plugins,
  pipewire,
  libva,
  ffmpeg-headless,
  openssl,
  libxkbcommon,
  wayland,
  fontconfig,
  freetype,
  curl,
  util-linux,
  wl-clipboard,
  wtype,
  makeWrapper,
  autoAddDriverRunpath,
  symlinkJoin,
  libx11,
  libxcb,
  libxcursor,
  libxrandr,
  libxi,
  mkShell,
  rust-analyzer,
  rustfmt,
  clippy,
}:

let
  buildEnv = {
    OPENSSL_NO_VENDOR = "1";
  };
  runtimeEnv = {
    LD_LIBRARY_PATH = lib.makeLibraryPath [
      # The window draws through wgpu, which opens libvulkan at runtime.
      vulkan-loader
      alsa-lib
      wayland
      libxkbcommon
      fontconfig
      # Linux Annotate dlopens libpipewire-0.3, libva and libva-drm. libva finds VA-API
      # drivers through its built-in /run/opengl-driver path or LIBVA_DRIVERS_PATH.
      pipewire
      libva
    ];
    ALSA_PLUGIN_DIR = symlinkJoin {
      name = "hex-alsa-plugins";
      paths = [
        "${pipewire}/lib/alsa-lib"
        "${alsa-plugins}/lib/alsa-lib"
      ];
    };
  };
  runtimePackages = [
    curl
    wl-clipboard
    wtype
    # Annotate runs ffmpeg and ffprobe; the headless build includes libx264.
    ffmpeg-headless
  ];
  # Every Git dependency Cargo.lock pins, with the revision its output hash was
  # computed from. scripts/check-nix-git-pins.py fails a pull request that bumps
  # one without the other, before Nix reports a hash mismatch.
  cargoGitDependencies = {
    "rune-0.0.1" = {
      rev = "ba5e7c8069a24c36f95ca90a3058083cd468fcd1";
      hash = "sha256-D0zeghVhWooVKQtxObxQDnGdMO0Zgo6uKrCWi7tVHEo=";
    };
  };
in
rustPlatform.buildRustPackage (finalAttrs: {
  pname = "hex";
  version = (builtins.fromTOML (builtins.readFile ../Cargo.toml)).package.version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../.cargo
      ../build.rs
      ../crates
      ../src
      ../tests
      ../native
      ../resources
      ../assets
      ../packaging
      ../macos/HexMicrophoneDriver/Info.plist
    ];
  };

  cargoLock = {
    lockFile = ../Cargo.lock;
    outputHashes = builtins.mapAttrs (_: dependency: dependency.hash) cargoGitDependencies;
  };

  nativeBuildInputs = [
    pkg-config
    python3
    makeWrapper
    autoAddDriverRunpath
  ];
  buildInputs = [
    vulkan-loader
    alsa-lib
    openssl
    libxkbcommon
    wayland
    fontconfig
    freetype
    libx11
    libxcb
    libxcursor
    libxrandr
    libxi
  ];

  # OpenCode HTTP requests run through curl, including in the tests; the
  # 2.3.1 adoption test (src/updater/legacy.rs) runs an installer script,
  # which needs flock.
  nativeCheckInputs = [
    curl
    util-linux
  ];

  env = buildEnv;
  # Desktop tests are #[ignore]; run the remaining suite.
  doCheck = true;
  preCheck = ''
    export HOME="$TMPDIR/hex-test-home"
    mkdir -p "$HOME"
  '';

  postInstall = ''
    install -Dm644 ${../packaging/anomaly.hex.desktop} $out/share/applications/anomaly.hex.desktop
    mkdir -p $out/share/icons
    cp -R ${../packaging/linux/icons/hicolor} $out/share/icons/hicolor
    substituteInPlace $out/share/applications/anomaly.hex.desktop \
      --replace-fail '@HEX_BIN@' "$out/bin/hex"
  '';

  postFixup = ''
    wrapProgram $out/bin/hex \
      --prefix PATH : ${lib.makeBinPath runtimePackages} \
      --prefix LD_LIBRARY_PATH : ${runtimeEnv.LD_LIBRARY_PATH} \
      --set ALSA_PLUGIN_DIR ${runtimeEnv.ALSA_PLUGIN_DIR}
  '';

  passthru.devShell = mkShell {
    inputsFrom = [ finalAttrs.finalPackage ];
    env = buildEnv // runtimeEnv;
    packages = runtimePackages ++ [
      rust-analyzer
      rustfmt
      clippy
    ];
  };

  meta = {
    description = "Voice dictation through ElevenLabs Scribe (personal fork of anomalyco/hex)";
    homepage = "https://github.com/awidor/hex";
    license = lib.licenses.mit;
    mainProgram = "hex";
    platforms = [
      "x86_64-linux"
      "aarch64-linux"
    ];
  };
})
