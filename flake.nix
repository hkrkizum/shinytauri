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

          # direnv (参考: direnv 本体はホスト側でも有効化が必要)
          direnv
          nix-direnv

          # Build tools
          pkg-config
          gcc
          clang
          mold
          gnumake

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
          rig = pkgs.callPackage ./nix/rig.nix { };
        in
        {
          default = pkgs.mkShell {
            packages = [
              rustToolchain
              rig
            ]
            ++ projectPackagesFor pkgs
            ++ fontPackagesFor pkgs;
            shellHook = ''
              export CODEX_HOME="$PWD/.codex"
              export PATH="$PWD/.rvenv/bin:$PATH:$HOME/.local/bin"
              export CC=clang
              export CXX=clang++
              export FONTCONFIG_FILE="${pkgs.makeFontsConf { fontDirectories = fontPackagesFor pkgs; }}"

              (
                [[ $- == *i* && -t 1 ]] || exit 0
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
                printf '\n'
              )
            '';
          };
        }
      );

      formatter = forAllSystems ({ pkgs }: pkgs.nixfmt-tree);
    };
}
