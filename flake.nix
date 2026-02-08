{
  description = "releasor2000";

  outputs = { self, nixpkgs }:
    let
      supportedSystems = {
          "x86_64-darwin" = mkPkg "x86_64-apple-darwin" "2566d1255f4c8af512085d7ac2739f90135b8b361908ff4a16a0f1dd7a08a918";
          "aarch64-darwin" = mkPkg "aarch64-apple-darwin" "88aefd07c05c009d0b945a8efb5b58283d0b8bc7e8ea989634d64de69f834be7";
          "x86_64-linux" = mkPkg "x86_64-unknown-linux-gnu" "ce16ab0c76b92d45c1001a887fc61caa8ec28ee107a03d7f763be60088c92f68";
          "aarch64-linux" = mkPkg "aarch64-unknown-linux-gnu" "e89082e83827dfad57eb7174f531c12ef46dc2a2235a1220ad9fe64607f366d4";
      };
    in {
      packages = builtins.mapAttrs (system: _:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          mkPkg = target: sha256: pkgs.stdenv.mkDerivation {
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
          pkg = supportedSystems.${system};
        in {
          releasor2000 = pkg;
          default = pkg;
        }
      ) supportedSystems;
    };
}
