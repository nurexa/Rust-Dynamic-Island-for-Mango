{
  description = "All-in-one Wayland desktop shell in one Dynamic Island pill: bar, launcher, notifications, clipboard, screenshots";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAll (pkgs: rec {
        island = pkgs.rustPlatform.buildRustPackage {
          pname = "island";
          version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;
          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;
          nativeBuildInputs = [ pkgs.pkg-config ];
          # The build sandbox has no fonts to shape text with.
          checkFlags = [ "--skip=pen::tests::cached_shaping_matches_a_fresh_shape" ];
          buildInputs = [ pkgs.cairo pkgs.pango pkgs.glib pkgs.libxkbcommon ];
          postInstall = ''
            install -Dm755 install.sh $out/share/island/install.sh
            install -Dm644 README.md MODDING.md -t $out/share/doc/island
            install -Dm644 examples/config.toml $out/share/doc/island/config.toml
            install -Dm644 LICENSE LICENSES/MIT-Phosphor.txt -t $out/share/licenses/island
          '';
          meta = {
            description = "All-in-one Wayland desktop shell in one Dynamic Island pill";
            homepage = "https://github.com/nurexa/Rust-Dynamic-Island";
            license = with pkgs.lib.licenses; [ gpl3Only asl20 ];
            mainProgram = "island";
            platforms = pkgs.lib.platforms.linux;
          };
        };
        default = island;
      });

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.system}.island ];
          packages = [ pkgs.cargo pkgs.rustc pkgs.clippy ];
        };
      });
    };
}
