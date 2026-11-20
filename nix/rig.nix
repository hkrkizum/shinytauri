{ stdenvNoCC, fetchurl }:
let
  version = "0.11.1";
  releases = {
    x86_64-linux = {
      platform = "linux-x86_64";
      hash = "sha256-dXtbeIawmoqX81q25xVGtoBE0yP/vKporcvoGECa8VY=";
    };
    aarch64-linux = {
      platform = "linux-aarch64";
      hash = "sha256-K3lohbwDmK53APjJWEeoVeKcNZdp8x4Zk1aSdDFvtQg=";
    };
    x86_64-darwin = {
      platform = "macos-x86_64";
      hash = "sha256-hDTpwRIFxmBqoKT9YZorsNr7JclRrS89nepfT8SKV5w=";
    };
    aarch64-darwin = {
      platform = "macos-arm64";
      hash = "sha256-7psezqqZOC4riKaeFLpMF40U3LXGQkG8WO8B0XUK/ks=";
    };
  };
  release = releases.${stdenvNoCC.hostPlatform.system};
in
stdenvNoCC.mkDerivation {
  pname = "r-rig";
  inherit version;
  src = fetchurl {
    url = "https://github.com/r-lib/rig/releases/download/v${version}/rig-${release.platform}-${version}.tar.gz";
    inherit (release) hash;
  };
  sourceRoot = ".";
  dontFixup = true;
  installPhase = ''
    mkdir -p "$out"
    cp -r bin share "$out/"
  '';
}
