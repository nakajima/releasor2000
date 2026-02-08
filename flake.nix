{
  description = "releasor2000";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: {
    packages = {
      "x86_64-darwin" = let
        pkgs = nixpkgs.legacyPackages.x86_64-darwin;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.0.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.5/releasor2000-0.0.5-x86_64-apple-darwin.tar.gz";
            sha256 = "ef40ddf8dbd917139e5265ed6894eaa083b0740b67f59f2d300e0488670b6b22";
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
          version = "0.0.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.5/releasor2000-0.0.5-aarch64-apple-darwin.tar.gz";
            sha256 = "e544258f2b2e27f6e0f6e315d3bb5de490acbc8f202f4f86c5e9862efaee867c";
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
          version = "0.0.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.5/releasor2000-0.0.5-x86_64-unknown-linux-gnu.tar.gz";
            sha256 = "c2ab15bb5160c7782817ca87e2e067773d08acd6a0896051791d4be78244841b";
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
          version = "0.0.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.5/releasor2000-0.0.5-aarch64-unknown-linux-gnu.tar.gz";
            sha256 = "9a7eb526a9116098ab300aacf303ba5a343cc999f909252cb25a5df93e183722";
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
