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

## Notes
- "Offline mode" only appears for accounts that were verified as owning the game.
- Account tokens are stored in `accounts.json` in the launcher data folder (same approach as the official launcher).
- Forge/NeoForge/Quilt are not included yet (Vanilla + Fabric keep the launcher tiny).
