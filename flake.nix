{
  description = "releasor2000";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: {
    packages = {
      "x86_64-darwin" = let
        pkgs = nixpkgs.legacyPackages.x86_64-darwin;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.1.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.8/releasor2000-0.1.8-x86_64-apple-darwin.tar.gz";
            sha256 = "bb28008bd086ac0b2a5f9dc917823cc870ebfbf24d1ace931570607ae87e733c";
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
          version = "0.1.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.8/releasor2000-0.1.8-aarch64-apple-darwin.tar.gz";
            sha256 = "e2ad2d6f49ace7ebb920e23858f069f3403eff555d6ff7c703485225e956ed70";
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
          version = "0.1.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.8/releasor2000-0.1.8-x86_64-unknown-linux-gnu.tar.gz";
            sha256 = "e186035780a98a9d8fff063267f5415ec6d0c61b1fdd699b766061245a589dfe";
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
          version = "0.1.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.1.8/releasor2000-0.1.8-aarch64-unknown-linux-gnu.tar.gz";
            sha256 = "6ca0b8e5b13286fdad6e6ed79c5d0065a1a78b51bbcba11bbfed5059eb13e2a2";
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
