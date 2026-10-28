# Radar assets

`bomb_ground.png` is the existing bundled C4 marker.

`agents.json` maps pawn model names to agent portraits. Metadata is a compact
extract of [ByMykel/CSGO-API agents.json](https://github.com/ByMykel/CSGO-API/blob/main/public/api/en/agents.json)
(MIT). Character artwork is owned by Valve and is fetched from the Valve Steam
CDN only when a matching model is known. The server caches PNGs locally. Unknown
models, offline mode without a cached portrait, or failed downloads use a neutral
silhouette; this is not the player's Steam profile avatar.

Equipment icons are local SVG silhouettes in `src/icons.rs` and work offline.
