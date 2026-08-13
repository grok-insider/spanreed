{
  description = "spanreed — Linux-native AI coding subscription usage tracker (daemon + CLI + Waybar)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
  };

  # Advertise the binary cache so `nix run/build github:grok-insider/spanreed` can
  # pull prebuilt closures instead of compiling. Users must trust these (Nix
  # will prompt, or add them to nix.settings on NixOS).
  nixConfig = {
    extra-substituters = [
      "https://grok-insider.cachix.org"
      "https://nix-community.cachix.org"
    ];
    extra-trusted-public-keys = [
      "grok-insider.cachix.org-1:8i89e8J7hJHfIBwZivzxY9Kt9fk89ywhAqW+ml7TOB4="
      "nix-community.cachix.org-1:mB9FSh9qf2dCimDSUo8Zy7bkq5CX+/rkCWyvRCYg3Fs="
    ];
  };

  outputs = { self, nixpkgs }:
    let
      lib = nixpkgs.lib;
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = lib.genAttrs systems;

      packageFor = system:
        let
          pkgs = import nixpkgs { inherit system; };

          # `secret-tool` (libsecret) is the only optional runtime dep: it is
          # used solely as a fallback when a provider stores its token in the
          # Secret Service rather than a plaintext file. We wrap it onto PATH so
          # the feature works out of the box, but the binary runs fine without
          # it (file-based credentials are the common case).
          runtimePath = lib.makeBinPath [
            pkgs.libsecret
            pkgs.xdg-utils
            pkgs.libnotify
          ];
        in
        pkgs.rustPlatform.buildRustPackage {
          pname = "spanreed";
          # Single source of truth: Cargo.toml (release-plz bumps it).
          version = (lib.importTOML ./Cargo.toml).package.version;
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          # GNU Linux desktop build: SNI tray (GTK3 + Ayatana). musl GH
          # release zips stay headless and are not this derivation.
          buildFeatures = [ "tray" ];

          nativeBuildInputs = [
            pkgs.makeBinaryWrapper
            pkgs.pkg-config
            pkgs.wrapGAppsHook3
            # rusqlite is built with the `bundled` feature, which compiles the
            # vendored SQLite amalgamation — needs a C toolchain at build time.
            pkgs.stdenv.cc
          ];

          buildInputs = [
            pkgs.gtk3
            pkgs.libayatana-appindicator
            pkgs.xdotool
          ];

          # wrapGAppsHook3 + wrapProgram: apply GLib/GTK env in our wrapper.
          dontWrapGApps = true;
          postFixup = ''
            wrapProgram "$out/bin/spanreed" \
              --prefix PATH : "${runtimePath}" \
              "''${gappsWrapperArgs[@]}"
          '';

          meta = {
            description = "Linux-native AI subscription usage tracker (daemon + CLI + Waybar)";
            mainProgram = "spanreed";
            license = lib.licenses.mit;
            platforms = systems;
          };
        };
    in
    {
      packages = forAllSystems (system: rec {
        default = packageFor system;
        spanreed = default;
      });

      apps = forAllSystems (system: {
        default = {
          type = "app";
          program = "${self.packages.${system}.default}/bin/spanreed";
        };
      });

      homeManagerModules.default = { config, lib, pkgs, ... }:
        let
          cfg = config.programs.spanreed;
        in
        {
          options.programs.spanreed = {
            enable = lib.mkEnableOption "spanreed AI subscription usage tracker";

            package = lib.mkOption {
              type = lib.types.package;
              default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
              defaultText = lib.literalExpression "spanreed.packages.\${pkgs.stdenv.hostPlatform.system}.default";
              description = "spanreed package to install.";
            };

            serve = {
              enable = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = ''
                  Run `spanreed serve` as a user service exposing the local
                  HTTP API on 127.0.0.1:6736.
                '';
              };

              interval = lib.mkOption {
                type = lib.types.int;
                default = 300;
                description = "Refresh interval in seconds for the serve daemon (min 30).";
              };
            };

            capture = {
              enable = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = ''
                  Run `spanreed capture serve` as a user service: dual reverse
                  proxies that record official Grok/xAI API usage for Last 30 Days.

                  Default binds:
                    127.0.0.1:18736 → cli-chat-proxy.grok.com  (Grok CLI)
                    127.0.0.1:18737 → api.x.ai                 (OpenCode xAI)

                  Point clients at those base URLs (wrappers / OpenCode baseURL).
                  Set egressProxy so upstream still uses your geo VPN (e.g. sing-box).
                '';
              };

              grokCliBind = lib.mkOption {
                type = lib.types.str;
                default = "127.0.0.1:18736";
                description = "Local bind for Grok CLI capture (upstream cli-chat-proxy.grok.com).";
              };

              xaiApiBind = lib.mkOption {
                type = lib.types.str;
                default = "127.0.0.1:18737";
                description = "Local bind for OpenCode/api.x.ai capture.";
              };

              egressProxy = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
                example = "http://127.0.0.1:7897";
                description = ''
                  Optional HTTP(S) proxy for capture→upstream egress (e.g. domain-only
                  sing-box for xAI). When set, the service exports HTTP_PROXY and
                  HTTPS_PROXY. Null means inherit ambient environment only.
                '';
              };
            };

            tray = {
              enable = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = ''
                  Run `spanreed tray` as a user service (Behelit SNI icon).
                  Requires a StatusNotifier host (Waybar `tray` on Hyprland).
                  Does not replace `spanreed waybar`.
                '';
              };

              interval = lib.mkOption {
                type = lib.types.int;
                default = 60;
                description = "Refresh interval in seconds for the tray (min 5).";
              };
            };
          };

          config = lib.mkIf cfg.enable {
            home.packages = [ cfg.package ];

            systemd.user.services.spanreed = lib.mkIf cfg.serve.enable {
              Unit = {
                Description = "spanreed local usage API";
                After = [ "graphical-session.target" ];
                PartOf = [ "graphical-session.target" ];
              };

              Service = {
                ExecStart = "${cfg.package}/bin/spanreed serve --interval ${toString cfg.serve.interval}";
                Restart = "on-failure";
                RestartSec = 5;
              };

              Install.WantedBy = [ "default.target" ];
            };

            systemd.user.services.spanreed-capture = lib.mkIf cfg.capture.enable {
              Unit = {
                Description = "spanreed Grok/xAI usage capture proxy";
                After = [ "network-online.target" ];
                Wants = [ "network-online.target" ];
              };

              Service = {
                ExecStart = lib.concatStringsSep " " [
                  "${cfg.package}/bin/spanreed"
                  "capture"
                  "serve"
                  "--grok-cli-bind"
                  cfg.capture.grokCliBind
                  "--xai-api-bind"
                  cfg.capture.xaiApiBind
                ];
                Restart = "on-failure";
                RestartSec = 3;
                Environment = lib.mkIf (cfg.capture.egressProxy != null) [
                  "HTTP_PROXY=${cfg.capture.egressProxy}"
                  "HTTPS_PROXY=${cfg.capture.egressProxy}"
                  "NO_PROXY=127.0.0.1,localhost,::1"
                ];
              };

              Install.WantedBy = [ "default.target" ];
            };

            systemd.user.services.spanreed-tray = lib.mkIf cfg.tray.enable {
              Unit = {
                Description = "spanreed system tray (Behelit)";
                After = [ "graphical-session.target" ];
                PartOf = [ "graphical-session.target" ];
              };

              Service = {
                ExecStart = "${cfg.package}/bin/spanreed tray --interval ${toString cfg.tray.interval}";
                Restart = "on-failure";
                RestartSec = 5;
              };

              Install.WantedBy = [ "graphical-session.target" ];
            };
          };
        };

      checks = forAllSystems (system: {
        default = self.packages.${system}.default;
      });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            packages = [
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.rust-analyzer
              pkgs.libsecret
              pkgs.pkg-config
              pkgs.gtk3
              pkgs.libayatana-appindicator
              pkgs.xdotool
              pkgs.libnotify
              pkgs.xdg-utils
            ];
          };
        });
    };
}
