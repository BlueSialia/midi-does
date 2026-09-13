{
  description = "midi-does: map MIDI controllers to PipeWire audio controls and shell commands";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      cargoToml = builtins.fromTOML (builtins.readFile ./Cargo.toml);
    in
    {
      packages = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          toolchain = pkgs.rust-bin.stable."1.97.1".minimal;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = toolchain;
            rustc = toolchain;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = cargoToml.package.name;
            version = cargoToml.package.version;

            src = builtins.filterSource
              (path: type:
                let base = baseNameOf path; in
                !(type == "directory" && (base == "target" || base == ".direnv" || base == ".git" || base == "result"))
              )
              ./.;

            cargoLock = {
              lockFile = ./Cargo.lock;
            };

            nativeBuildInputs = with pkgs; [ pkg-config libclang ];
            buildInputs = with pkgs; [
              pipewire
              alsa-lib
              vulkan-loader
              libxkbcommon
              libx11
              libxcb
              libxcursor
              libxrandr
              libxi
              libxext
              libxinerama
              libxxf86vm
              wayland
            ];

            LIBCLANG_PATH = "${pkgs.libclang.lib}/lib";
            BINDGEN_EXTRA_CLANG_ARGS = "-isystem ${pkgs.glibc.dev}/include";

            meta = {
              description = "Map MIDI controllers to PipeWire audio controls and shell commands";
              homepage = "https://github.com/BlueSialia/midi-does";
              license = {
                shortName = "Hippocratic-3.0";
                fullName = "Hippocratic License 3.0";
                url = "https://firstdonoharm.dev/";
                free = true;
              };
              mainProgram = "midi-does";
              platforms = pkgs.lib.platforms.linux;
            };
          };
        });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs {
            inherit system;
            overlays = [ (import rust-overlay) ];
          };

          runtimeLibs = with pkgs; [
            vulkan-loader
            libxkbcommon
            libx11
            libxcb
            libxcursor
            libxrandr
            libxi
            libxext
            libxinerama
            libxxf86vm
            wayland
            alsa-lib
            pipewire
            udev
          ];
        in
        {
          default = pkgs.mkShell {
            buildInputs = with pkgs; [
              rust-bin.stable."1.97.1".default
              pkg-config
              pipewire
              alsa-lib
              udev
              vulkan-loader
              libxkbcommon
              libx11
              libxcb
              libxcursor
              libxrandr
              libxi
              libxext
              libxinerama
              libxxf86vm
              wayland
              libclang
            ];

            shellHook = ''
              export PATH="$HOME/.local/share/mise/shims:$PATH"
              export LIBCLANG_PATH="${pkgs.libclang.lib}/lib"
              export BINDGEN_EXTRA_CLANG_ARGS="-isystem ${pkgs.glibc.dev}/include"
              export C_INCLUDE_PATH="${pkgs.glibc.dev}/include"
            '';

            LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath runtimeLibs;
          };
        });
    };
}
