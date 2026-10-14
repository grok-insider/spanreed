{ pkgs, manifest, lockfile }:
let
  lib = pkgs.lib;
  registry = builtins.filter (p: (p.source or "") == "registry+https://github.com/rust-lang/crates.io-index") (lib.importTOML lockfile).package;
  archives = builtins.listToAttrs (map (p: {
    name = "${p.name}-${p.version}.crate";
    value = toString (pkgs.fetchurl {
      url = "https://static.crates.io/crates/${p.name}/${p.name}-${p.version}.crate";
      sha256 = p.checksum;
    });
  }) registry);
  upstream = builtins.fromJSON (builtins.readFile ../scripts/notices/upstream-inputs.json);
  offlineInputs = lib.mapAttrs (url: sha256: {
    inherit sha256;
    file = toString (pkgs.fetchurl { inherit url sha256; });
  }) upstream;
in {
  archiveIndex = pkgs.writeText "spanreed-native-crate-archives.json" (builtins.toJSON archives);
  inputIndex = pkgs.writeText "spanreed-native-notice-inputs.json" (builtins.toJSON offlineInputs);
  rustSource = pkgs.rustc.unwrapped.src;
  target = pkgs.stdenv.hostPlatform.rust.rustcTarget;
  inherit manifest;
}
