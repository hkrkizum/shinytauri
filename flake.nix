{
  description = "Development shell for shinytauri";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    { nixpkgs, rust-overlay, ... }:
    let
      systems = [
        "x86_64-linux"
      ];
      forAllSystems =
        f:
        nixpkgs.lib.genAttrs systems (
          system:
          f {
            pkgs = import nixpkgs {
              inherit system;
              overlays = [ rust-overlay.overlays.default ];
            };
          }
        );
      fontPackagesFor =
        pkgs: with pkgs; [
          noto-fonts-cjk-sans
          noto-fonts-cjk-serif
          noto-fonts-color-emoji
          nerd-fonts.meslo-lg
          plemoljp-nf
        ];
      projectPackagesFor =
        pkgs: with pkgs; [
          # JS toolchains
          nodejs
          pnpm

          # Nix
          nixd # Nix LSP
          nixfmt # Nix フォーマッタ

          # Development shell, task runner and QA
          quarto
          python3
          nushell
          nufmt
          nu-lint
          mise
          git
          fd
          statix
          taplo
          air-formatter
          jarl
          # rig's R adds host libraries to LD_LIBRARY_PATH. Chromium needs Nix's GLib.
          (writeShellScriptBin "chromium" ''
            unset LD_LIBRARY_PATH
            exec ${lib.getExe chromium} "$@"
          '')

          # direnv (参考: direnv 本体はホスト側でも有効化が必要)
          direnv
          nix-direnv

          # Build tools
          pkg-config
          gcc
          clang
          mold
          gnumake

          # Tauri's Linux WebView and native menu
          webkitgtk_4_1
          xdotool

          # Rust tools
          cargo-nextest
          cargo-make
          cargo-llvm-cov
          cargo-tauri
        ];

    in
    {
      devShells = forAllSystems (
        { pkgs }:
        let
          rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
          rustVersion = (builtins.fromTOML (builtins.readFile ./rust-toolchain.toml)).toolchain.channel;
          windowsRust = pkgs.rust-bin.stable.${rustVersion}.minimal.override {
            targets = [ "x86_64-pc-windows-gnu" ];
          };
          windowsBuildTools = with pkgs; [
            windowsRust
            stdenv.cc # Native build scripts and procedural macros
            pkgsCross.mingwW64.stdenv.cc
            nsis
            git
          ];
          # Tauri probes `rustup target list` on PATH. Only expose the Nix
          # toolchain and its native tools to this subprocess: a host rustup
          # describes a different sysroot from the compiler doing the build.
          windowsTauri = pkgs.writeShellScriptBin "cargo-tauri" ''
            export PATH=${pkgs.lib.makeBinPath (windowsBuildTools ++ pkgs.stdenv.initialPath)}
            exec ${pkgs.cargo-tauri}/bin/cargo-tauri "$@"
          '';
          rig = pkgs.callPackage ./nix/rig.nix { };
        in
        {
          # Manual pages use static Markdown and need no R or Jupyter engine.
          # flake.lock pins nixpkgs; check the renderer when updating the lock.
          docs =
            assert pkgs.lib.assertMsg (
              pkgs.quartoMinimal.version == "1.10.18"
            ) "Review the manual with the new Quarto version before updating the pin";
            pkgs.mkShellNoCC {
              packages = with pkgs; [
                quartoMinimal
                python3
                mise
                nushell
              ];
            };
          windows = pkgs.mkShell {
            packages =
              windowsBuildTools
              ++ (with pkgs; [
                windowsTauri
                rig
                nushell
                mise
              ]);
            env = {
              SHINYTAURI_CARGO_TAURI = "${windowsTauri}/bin/cargo-tauri";
              SHINYTAURI_WINDOWS_SHELL = "1";
              R_LD_LIBRARY_PATH = "$ORIGIN/../../lib";
              NIX_LDFLAGS_x86_64_w64_mingw32 = "-L${pkgs.pkgsCross.mingwW64.windows.pthreads}/lib";
              CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER = "x86_64-w64-mingw32-gcc";
            };
          };
          default = pkgs.mkShell {
            env.RIG_MODE = "user";
            packages = [
              rustToolchain
              rig
            ]
            ++ projectPackagesFor pkgs
            ++ fontPackagesFor pkgs;
            shellHook = ''
              export CODEX_HOME="$PWD/.codex"
              export PATH="$PWD/src-shiny/.rvenv/bin:$PATH:$HOME/.local/bin"
              # rig's R needs its own libR.so. Adding the host libc directory
              # to LD_LIBRARY_PATH would also load it into Nix subprocesses.
              export R_LD_LIBRARY_PATH='$ORIGIN/../../lib'
              export CC=clang
              export CXX=clang++
              export FONTCONFIG_FILE="${pkgs.makeFontsConf { fontDirectories = fontPackagesFor pkgs; }}"

              if [[ $- == *i* && -t 0 && -t 1 ]]; then
                accent= reset=
                if [[ -z ''${NO_COLOR:-} && ''${TERM:-dumb} != dumb ]]; then
                  accent=$'\033[1;36m'
                  reset=$'\033[0m'
                fi

                row() { printf '  %s│%s  %-6s  %s\n' "$accent" "$reset" "$1" "$2"; }
                if ! r_version=$(Rscript --vanilla --default-packages=NULL -e 'cat(as.character(getRversion()))' 2>/dev/null); then
                  r_version='利用不可 (rig list で確認)'
                fi

                printf '\n  %s◆ shinytauri%s  ·  ${pkgs.stdenv.hostPlatform.system}\n' "$accent" "$reset"
                row Rust  'rustc ${rustToolchain.version} · cargo'
                row Cargo 'tauri ${pkgs.cargo-tauri.version} · nextest · make · llvm-cov'
                row R     "$r_version · rig ${rig.version}"
                row Web   'Node.js ${pkgs.nodejs.version} · pnpm ${pkgs.pnpm.version}'
                row Build 'clang · gcc · mold · make · pkg-config'
                row Nix   'nixd · nixfmt · direnv · nix-direnv'
                row QA    'mise run QA · statix · taplo · air · jarl'
                printf '\n'

                exec "${pkgs.lib.getExe pkgs.nushell}" --execute '$env.SHELL = $nu.current-exe'
              fi
            '';
          };
        }
      );

      formatter = forAllSystems ({ pkgs }: pkgs.nixfmt-tree);
    };
}
