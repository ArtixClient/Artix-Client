# Artix Launcher

A very light Minecraft launcher for low-end PCs. One small native program (no Electron, no Java needed to run the launcher itself).

## What it does
- **Bundles Artix Client**: every Fabric instance gets your `artix-client.jar` automatically, re-synced right before every launch (so a freshly built jar is picked up with no extra step). Point Settings at the jar once, or just drop `ArtixClient.jar` next to the launcher's exe and it's found automatically. New instances default to Fabric for this reason.
- **Ultimate performance mode**: a 4th memory mode using Aikar's well-known G1GC tuning flags (low pause times, steadier frame times), plus a "Fixed memory" option (start = max heap, no mid-game resizing) and a one-click "Best performance preset" button per instance.
- **Separate instances**: every instance lives in its own folder (default `%APPDATA%\ArtixLauncher\instances`), never in `.minecraft`. You can move the instances folder anywhere (Settings).
- **Vanilla + Fabric**, any release/snapshot version. Libraries and assets are shared between instances to save disk.
- **No Java install needed**: downloads Mojang's official Java runtime for each version automatically.
- **Memory and resolution** per instance, fullscreen, low-memory (Serial GC) mode, extra JVM args, custom Java path. Detects your RAM and recommends a safe amount.
- **Performance mods in one click** (Fabric instances): Sodium, Lithium, FerriteCore, ModernFix, Entity Culling, ImmediatelyFast + Fabric API, from Modrinth.
- **Skin editor**: pixel editor with front/back preview, slim/classic, undo/redo, import from any username, open/save PNG, apply to your account, switch capes.
- **Close-on-launch** to free RAM while you play.
- **Official sign-in only** (see below). Nothing is ever sent to the launcher's author; there is no server.

## Microsoft sign-in (one-time setup for YOU, the developer)
Minecraft only accepts logins from apps registered with Microsoft. This is free:
1. Go to https://portal.azure.com -> **App registrations** -> **New registration**. Name: Artix Launcher. Supported account types: **Personal Microsoft accounts only**. Redirect URI: leave empty.
2. Open the app -> **Authentication** -> **Advanced settings** -> **Allow public client flows = Yes** -> Save.
3. Copy the **Application (client) ID**.
4. Request Minecraft API access for that ID using Mojang's form (https://aka.ms/AppRegInfo). Until it is approved, Minecraft login returns 403 ("Invalid app registration").
5. Build with your ID: `ARTIX_CLIENT_ID=<id> cargo build --release` (Windows PowerShell: `$env:ARTIX_CLIENT_ID="<id>"; cargo build --release`). For GitHub Actions, add it as a repository secret named `ARTIX_CLIENT_ID`.

Players then sign in with the normal Microsoft device-code page (microsoft.com/link). The launcher checks ownership through Minecraft Services' official profile endpoint.

## Build
Install Rust (https://rustup.rs), then `cargo build --release`. The exe is in `target/release/`.
No Rust? Push this folder to GitHub: the included workflow builds Windows/Linux/macOS binaries (Actions tab -> artifacts).

## Notes
- "Offline mode" only appears for accounts that were verified as owning the game.
- Account tokens are stored in `accounts.json` in the launcher data folder (same approach as the official launcher).
- Forge/NeoForge/Quilt are not included yet (Vanilla + Fabric keep the launcher tiny).
