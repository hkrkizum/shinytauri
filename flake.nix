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
          (rust-bin.fromRustupToolchainFile ./rust-toolchain.toml)
          cargo-nextest
          cargo-make
          cargo-llvm-cov
          cargo-tauri

          # R Installation Manager
          (callPackage ./nix/rig.nix { })
        ];

    in
    {
      devShells = forAllSystems (
        { pkgs }:
        {
          default = pkgs.mkShell {
            packages = (projectPackagesFor pkgs) ++ (fontPackagesFor pkgs);
            shellHook = ''
              export CODEX_HOME="$PWD/.codex"
              export PATH="$PWD/.rvenv/bin:$PATH:$HOME/.local/bin"
              export CC=clang
              export CXX=clang++
              export FONTCONFIG_FILE="${pkgs.makeFontsConf { fontDirectories = fontPackagesFor pkgs; }}"
            '';
          };
        }
      );

      formatter = forAllSystems ({ pkgs }: pkgs.nixfmt-tree);
    };
}
