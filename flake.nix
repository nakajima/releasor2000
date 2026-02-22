{
  description = "releasor2000";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";

  outputs = { self, nixpkgs }: {
    packages = {
      "x86_64-darwin" = let
        pkgs = nixpkgs.legacyPackages.x86_64-darwin;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.0.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.8/releasor2000-0.0.8-x86_64-apple-darwin.tar.gz";
            sha256 = "8533b7d027c66626e41880104f23a6f477b7779ae171ff3b25b66775a79b498a";
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
          version = "0.0.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.8/releasor2000-0.0.8-aarch64-apple-darwin.tar.gz";
            sha256 = "0f5df4b19f25060d560607b768bad0eaa39f3c175fa438dbf2e5f5d0e49b3f1a";
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
          version = "0.0.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.8/releasor2000-0.0.8-x86_64-unknown-linux-gnu.tar.gz";
            sha256 = "328d9f7832a65a6e3eba747f208e42886c4d4631d84d7f30d1bb1db247dcb670";
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
          version = "0.0.8";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.8/releasor2000-0.0.8-aarch64-unknown-linux-gnu.tar.gz";
            sha256 = "a4ae921b4763ba30978aeabbe09aaeb92fcae57b4f1baa9f1bbca0ae4ca451d4";
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
