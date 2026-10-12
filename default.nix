{ pkgs, crane }:
let
  inherit (pkgs) lib;
  craneLib = crane.mkLib pkgs;

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      (craneLib.fileset.commonCargoSources ./.)
      ./web/src/bindings
    ];
  };

  commonArgs = {
    pname = "lodestar";
    version = "0.1.0";
    inherit src;
    strictDeps = true;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;

  mkBin =
    crate: extra:
    craneLib.buildPackage (
      commonArgs
      // {
        inherit cargoArtifacts;
        pname = crate;
        cargoExtraArgs = "-p ${crate}";
        doCheck = false;
      }
      // extra
    );

  # The dashboard bundle (Vite build output), embedded into lodestar-server.
  lodestar-web = pkgs.buildNpmPackage {
    pname = "lodestar-web";
    version = "0.1.0";
    # Flake sources never contain the git-ignored node_modules/ and dist/.
    src = ./web;
    npmDepsHash = "sha256-Ry6RH2Kod1bShU3orxHvvsuGc3Hia3gmReZD7krChSs=";
    installPhase = ''
      runHook preInstall
      cp -r dist $out
      runHook postInstall
    '';
  };
in
{
  inherit lodestar-web;
  lodestar-agent = mkBin "lodestar-agent" { meta.mainProgram = "lodestar-agent"; };
  lodestar = mkBin "lodestar-cli" { meta.mainProgram = "lodestar"; };
  lodestar-server = mkBin "lodestar-server" {
    LODESTAR_WEB_DIST = "${lodestar-web}";
    meta.mainProgram = "lodestar-server";
  };

  bindingsCheck = craneLib.cargoTest (
    commonArgs
    // {
      inherit cargoArtifacts;
      pname = "lodestar-bindings";
      cargoTestExtraArgs = "-p lodestar-proto";
      preCheck = ''
        export TS_RS_EXPORT_DIR="$TMPDIR/b"
      '';
      postCheck = ''
        diff -r "$TMPDIR/b" web/src/bindings
      '';
    }
  );
}
