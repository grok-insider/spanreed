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

      desktopFor = system:
        let
          pkgs = import nixpkgs { inherit system; };
          version = (lib.importTOML ./desktop/src-tauri/Cargo.toml).package.version;
          # Keep source builds independent from local node_modules, target and dist.
          source = lib.cleanSourceWith {
            src = ./.;
            filter = path: type:
              !(builtins.elem (builtins.baseNameOf path) [ "target" "node_modules" "dist" ".git" ]);
          };
          nodeModules = pkgs.stdenvNoCC.mkDerivation {
            pname = "spanreed-desktop-node-modules";
            inherit version;
            src = source;
            nativeBuildInputs = [ pkgs.bun pkgs.cacert ];
            dontConfigure = true;
            dontFixup = true;
            buildPhase = ''
              export HOME="$TMPDIR/home"
              mkdir -p "$HOME"
              export BUN_INSTALL_CACHE_DIR="$TMPDIR/bun-cache"
              cd desktop
              bun install --frozen-lockfile --ignore-scripts --no-progress --cpu '*' --os linux
            '';
            installPhase = ''
              cp -R node_modules "$out"
            '';
            outputHashMode = "recursive";
            outputHashAlgo = "sha256";
            outputHash = "sha256-8jVjt6lU36rzS0xZkrbfifZdCoP2IgwaRnIj4GobhcM=";
          };
        in pkgs.rustPlatform.buildRustPackage {
          pname = "spanreed-desktop";
          inherit version;
          src = source;
          cargoRoot = "desktop/src-tauri";
          buildAndTestSubdir = "desktop/src-tauri";
          cargoLock.lockFile = ./desktop/src-tauri/Cargo.lock;
          buildFeatures = [ "tauri/custom-protocol" ];
          nativeBuildInputs = [ pkgs.bun pkgs.nodejs pkgs.pkg-config pkgs.wrapGAppsHook3 pkgs.makeWrapper ];
          buildInputs = [ pkgs.gtk3 pkgs.webkitgtk_4_1 pkgs.libayatana-appindicator pkgs.openssl pkgs.librsvg pkgs.glib-networking ];
          preBuild = ''
            cp -R ${nodeModules} desktop/node_modules
            chmod -R u+w desktop/node_modules
            # Local UI source must not come from the fixed-output dependency cache.
            rm -rf desktop/node_modules/@fabrials/ui
            ln -s ../../vendor/fabrials-ui desktop/node_modules/@fabrials/ui
            patchShebangs desktop/node_modules
            (cd desktop && bun run build)
          '';
          postInstall = ''
            install -Dm644 desktop/src-tauri/icons/128x128.png "$out/share/icons/hicolor/128x128/apps/com.fabrials.spanreed.png"
            mkdir -p "$out/share/applications"
            cat > "$out/share/applications/com.fabrials.spanreed.desktop" <<EOF
            [Desktop Entry]
            Type=Application
            Name=Spanreed
            Comment=Your AI accounts and usage, on your machine
            Exec=spanreed-desktop
            Icon=com.fabrials.spanreed
            Categories=Development;Utility;
            Terminal=false
            StartupWMClass=spanreed-desktop
            EOF
          '';
          dontWrapGApps = true;
          postFixup = ''
            wrapProgram "$out/bin/spanreed-desktop" \
              --prefix PATH : "${lib.makeBinPath [ pkgs.libsecret pkgs.xdg-utils pkgs.libnotify ]}" \
              --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [ pkgs.libayatana-appindicator pkgs.gtk3 ]}" \
              "''${gappsWrapperArgs[@]}"
          '';
          passthru = { inherit nodeModules; };
          meta = {
            description = "Local AI accounts, usage and routing console";
            homepage = "https://fabrials.com";
            mainProgram = "spanreed-desktop";
            license = lib.licenses.mit;
            platforms = systems;
          };
        };

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
          # tray-icon dlopens Ayatana at runtime (not a link-time NEEDED).
          trayLibPath = lib.makeLibraryPath [
            pkgs.libayatana-appindicator
            pkgs.gtk3
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
              --prefix LD_LIBRARY_PATH : "${trayLibPath}" \
              "''${gappsWrapperArgs[@]}"
          '';

          meta = {
            description = "Linux-native AI subscription usage tracker (daemon + CLI + Waybar + tray)";
            homepage = "https://github.com/grok-insider/spanreed";
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
        spanreed-desktop = desktopFor system;
      });

      overlays.default = final: prev: {
        spanreed = self.packages.${prev.stdenv.hostPlatform.system}.default;
        spanreed-desktop = self.packages.${prev.stdenv.hostPlatform.system}.spanreed-desktop;
      };

      formatter = forAllSystems (system: (import nixpkgs { inherit system; }).nixfmt-rfc-style);

      apps = forAllSystems (system: {
        desktop = {
          type = "app";
          program = "${self.packages.${system}.spanreed-desktop}/bin/spanreed-desktop";
        };
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

            desktop = {
              enable = lib.mkEnableOption "Spanreed Desktop local console";
              package = lib.mkOption {
                type = lib.types.package;
                default = self.packages.${pkgs.stdenv.hostPlatform.system}.spanreed-desktop;
                defaultText = lib.literalExpression "spanreed.packages.\${pkgs.stdenv.hostPlatform.system}.spanreed-desktop";
                description = "Spanreed Desktop package to install alongside the CLI.";
              };
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
                  Run `spanreed capture serve` as a user service: one fabric on
                  grokCliBind (default 127.0.0.1:18736) that records official
                  Grok/xAI API usage for Last 30 Days.

                    /v1        → cli-chat-proxy.grok.com  (Grok Build, SuperGrok inject)
                    /xai/v1    → api.x.ai                 (OpenCode, client token)
                    /acct/ID/… → same, pinned account

                  Point Grok Build at http://127.0.0.1:18736/v1 and OpenCode
                  provider.xai.options.baseURL at http://127.0.0.1:18736/xai/v1.
                  Set egressProxy so upstream still uses your geo VPN (e.g. sing-box).
                '';
              };

              grokCliBind = lib.mkOption {
                type = lib.types.str;
                default = "127.0.0.1:18736";
                description = "Local bind for the capture fabric (path selects upstream).";
              };

              xaiApiBind = lib.mkOption {
                type = lib.types.nullOr lib.types.str;
                default = null;
                example = "127.0.0.1:18737";
                description = ''
                  Optional compat listener that treats bare /v1 as /xai/v1
                  (old OpenCode configs on :18737). Null (default): fabric only.
                '';
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
                  Run `spanreed tray` as a user service (Spanreed SNI icon).
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
            home.packages = [ cfg.package ] ++ lib.optionals cfg.desktop.enable [ cfg.desktop.package ];

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
                ExecStart = lib.concatStringsSep " " (
                  [
                    "${cfg.package}/bin/spanreed"
                    "capture"
                    "serve"
                    "--grok-cli-bind"
                    cfg.capture.grokCliBind
                  ]
                  ++ lib.optionals (cfg.capture.xaiApiBind != null) [
                    "--xai-api-bind"
                    cfg.capture.xaiApiBind
                  ]
                );
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
                Description = "spanreed system tray (Spanreed)";
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
        desktop = self.packages.${system}.spanreed-desktop;
      });

      devShells = forAllSystems (system:
        let
          pkgs = import nixpkgs { inherit system; };
        in
        {
          default = pkgs.mkShell {
            shellHook = ''
              export LD_LIBRARY_PATH="${lib.makeLibraryPath [ pkgs.libayatana-appindicator pkgs.gtk3 ]}''${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
            '';
            packages = [
              pkgs.bun
              pkgs.cargo
              pkgs.rustc
              pkgs.rustfmt
              pkgs.clippy
              pkgs.rust-analyzer
              pkgs.libsecret
              pkgs.pkg-config
              pkgs.gtk3
              pkgs.webkitgtk_4_1
              pkgs.dbus
              pkgs.openssl
              pkgs.librsvg
              pkgs.libayatana-appindicator
              pkgs.xdotool
              pkgs.libnotify
              pkgs.xdg-utils
            ];
          };
        });
    };
}
