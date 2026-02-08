{
  description = "releasor2000";

  outputs = { self, nixpkgs }: let
    mkPkg = { target, sha256 }: let
      pkgs = nixpkgs.legacyPackages.${"$"}{system};
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
            sha256 = "68826491c132c87392c407082f9064d79ffc1f5462a68935f0392a89f125c3b9";
          };
          "aarch64-darwin" = mkPkg {
            target = "aarch64-apple-darwin";
            sha256 = "552141cb7e6a4e1568b0d49118effebee6a7a5b102e9b74d268b4f688c78e249";
          };
          "x86_64-linux" = mkPkg {
            target = "x86_64-unknown-linux-gnu";
            sha256 = "1a9f668d89acfe8f7a8fb86c1d68e673c9fbe37fbe063fd9f4ce1c8dcada3e58";
          };
          "aarch64-linux" = mkPkg {
            target = "aarch64-unknown-linux-gnu";
            sha256 = "7c333db2f2054088654c111c2e604e7823220c9c4922da3130c5ab7be99998a4";
          };
    };
  in {
    packages = builtins.mapAttrs (system: mkPkg: {
      releasor2000 = mkPkg;
      default = mkPkg;
    }) systems;
  };
}
