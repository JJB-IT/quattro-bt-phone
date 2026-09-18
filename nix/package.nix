{ lib, rustPlatform }:

let
  cargo = lib.importTOML ../Cargo.toml;
in
rustPlatform.buildRustPackage {
  pname = "quattro-bt-phone";
  inherit (cargo.workspace.package) version;

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../crates
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  meta = {
    description = "Make and take phone calls from Omarchy via a Bluetooth-paired phone";
    homepage = "https://github.com/JJB-IT/quattro-bt-phone";
    license = lib.licenses.mit;
    mainProgram = "quattro-bt-phone";
    platforms = lib.platforms.linux;
  };
}
