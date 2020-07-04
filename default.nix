{ pkgs ? import <nixpkgs> {}
, lib ? pkgs.lib
, rustPlatform ? pkgs.rustPlatform
, openssl ? pkgs.openssl
, pkgconfig ? pkgs.pkgconfig
}:
rustPlatform.buildRustPackage rec {
  pname = "nix-mirror";
  version = "0.1";

  nativeBuildInputs = [ pkgconfig ];
  buildInputs = [ openssl.dev ];

  src = lib.cleanSource ./.;
  cargoSha256 = "0dm9nmz8qblj2s67jy6gcsx9hqqkh42nk64yd8zppmky06p2x939";
}
