{
  description = "releasor2000";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: {
    packages = {
      "x86_64-darwin" = let
        pkgs = nixpkgs.legacyPackages.x86_64-darwin;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.1.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.5/releasor2000-0.1.5-x86_64-apple-darwin.tar.gz";
            sha256 = "69e1ef7be01565df9999d22df5a8550ec0e28fc3761841198a0968f79778856a";
          };
          sourceRoot = ".";
          installPhase = ''
            install -m755 -D releasor2000 $out/bin/releasor2000
          '';
        };
      in { releasor2000 = pkg; default = pkg; };
      "aarch64-darwin" = let
        pkgs = nixpkgs.legacyPackages.aarch64-darwin;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.1.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.5/releasor2000-0.1.5-aarch64-apple-darwin.tar.gz";
            sha256 = "916abe2a33d8bf69ee75b2b4d3cabec3706dfb32382e05d2c47f657d8d4989a5";
          };
          sourceRoot = ".";
          installPhase = ''
            install -m755 -D releasor2000 $out/bin/releasor2000
          '';
        };
      in { releasor2000 = pkg; default = pkg; };
      "x86_64-linux" = let
        pkgs = nixpkgs.legacyPackages.x86_64-linux;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.1.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.5/releasor2000-0.1.5-x86_64-unknown-linux-gnu.tar.gz";
            sha256 = "a569b2961cc2eddaef3f4cb60d32cad0c8e6dd0f16334ae99d10bd9c48344cc3";
          };
          sourceRoot = ".";
          installPhase = ''
            install -m755 -D releasor2000 $out/bin/releasor2000
          '';
        };
      in { releasor2000 = pkg; default = pkg; };
      "aarch64-linux" = let
        pkgs = nixpkgs.legacyPackages.aarch64-linux;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.1.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.5/releasor2000-0.1.5-aarch64-unknown-linux-gnu.tar.gz";
            sha256 = "87e787f2bfc01440e99e283dc8ea37ecb004f08853a003cf25fe71ab4b2b7598";
          };
          sourceRoot = ".";
          installPhase = ''
            install -m755 -D releasor2000 $out/bin/releasor2000
          '';
        };
      in { releasor2000 = pkg; default = pkg; };
    };
  };
}
