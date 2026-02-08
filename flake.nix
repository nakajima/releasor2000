{
  description = "releasor2000";

  outputs = { self, nixpkgs }: let
    mkPkg = { target, sha256 }: let
      pkgs = nixpkgs.legacyPackages.${system};
    in pkgs.stdenv.mkDerivation {
      pname = "releasor2000";
      version = "0.0.5";
      src = pkgs.fetchurl {
        url = "https://github.com/nakajima/releasor2000/releases/download/v0.0.5/releasor2000-0.0.5-${target}.tar.gz";
        inherit sha256;
      };
      sourceRoot = ".";
      installPhase = ''
        install -m755 -D releasor2000 $out/bin/releasor2000
      '';
    };
    systems = {
          "x86_64-darwin" = mkPkg {
            target = "x86_64-apple-darwin";
            sha256 = "ba9483eae30d2db136d04a4a3a76fbe6da66da108dade8af992fb831baf0dba9";
          };
          "aarch64-darwin" = mkPkg {
            target = "aarch64-apple-darwin";
            sha256 = "3ea818ef7356283f53604e745d5121f109387fcab10a0dd68590c22a44ba03fa";
          };
          "x86_64-linux" = mkPkg {
            target = "x86_64-unknown-linux-gnu";
            sha256 = "51a45acfba56034cbf8e7e449b128acfa2c0f9dd0831939c05676bffc5e0e056";
          };
          "aarch64-linux" = mkPkg {
            target = "aarch64-unknown-linux-gnu";
            sha256 = "1da5a5a588bf4ab4a75d5fff3b32c23e5b78944084643c5f0d67376c10f92488";
          };
    };
  in {
    packages = builtins.mapAttrs (system: mkPkg: {
      releasor2000 = mkPkg;
      default = mkPkg;
    }) systems;
  };
}
