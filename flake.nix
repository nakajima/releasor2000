{
  description = "releasor2000";

  outputs = { self, nixpkgs }: {
    packages = {
      "x86_64-darwin" = let
        pkgs = nixpkgs.legacyPackages.x86_64-darwin;
        pkg = pkgs.stdenv.mkDerivation {
          pname = "releasor2000";
          version = "0.0.5";
          src = pkgs.fetchurl {
            url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.5/releasor2000-0.0.5-x86_64-apple-darwin.tar.gz";
            sha256 = "d67f5a1423763b4cdb8482aa35d118608e73d62a385d09853ec58e2e197eb0a5";
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
            sha256 = "3fe849cc240d5402fd479a66c55426a6c567f3846ed3d363442eea5191f64aa1";
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
            sha256 = "d74595fef0508132831f712f24a078f5b81fdf62ce78269850f389bc32cc5c2d";
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
            sha256 = "08d90cb51e64c972716bbeed71ac8678817f824071cb633f76f6d63e96665935";
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
