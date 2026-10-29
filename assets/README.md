# Radar assets

`bomb_ground.png` is the existing bundled C4 marker.

`agents.json` maps pawn model names to agent portraits. Metadata is a compact
extract of [ByMykel/CSGO-API agents.json](https://github.com/ByMykel/CSGO-API/blob/main/public/api/en/agents.json)
(MIT). Character artwork is owned by Valve and is fetched from the Valve Steam
CDN only when a matching model is known. The server caches PNGs locally. Unknown
models, offline mode without a cached portrait, or failed downloads use a neutral
silhouette; this is not the player's Steam profile avatar.

Equipment icons are local SVG silhouettes in `src/icons.rs` and work offline.

`schema-bundle.json` is a complete fallback for first launch without network
access. Its offsets, client schema and build information were fetched together
from [a2x/cs2-dumper revision 1005c258412a3657ac73ed1a9f84b9cbedddd876](https://github.com/a2x/cs2-dumper/tree/1005c258412a3657ac73ed1a9f84b9cbedddd876/output)
(build 14192). The upstream MIT notice is in `schema_LICENSE.txt`. Runtime
updates still resolve one immutable revision before fetching all three files;
unrelated loose cache files are never combined into a schema.
