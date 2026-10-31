# Radar assets

`bomb_ground.png` is the existing bundled C4 marker.

Player cards use Steam profile avatars, resolved by SteamID64 through Steam's
public profile XML without an API key. Images are cached locally as
`steam_avatar_<steamid>.img` and remain available offline. Profiles are refreshed
after 24 hours; failures retain an existing avatar or show a neutral silhouette.

`agents.json` maps pawn model names to agent portraits for the legacy
`/api/agent-icon` endpoint. Player cards do not use these portraits. Metadata is a compact
extract of [ByMykel/CSGO-API agents.json](https://github.com/ByMykel/CSGO-API/blob/main/public/api/en/agents.json)
(MIT). Character artwork is owned by Valve and is fetched from the Valve Steam
CDN only when a matching model is known. The server caches PNGs locally. Unknown
models, offline mode without a cached portrait, or failed downloads use a neutral
silhouette; this is not the player's Steam profile avatar.

Equipment icons are local SVG silhouettes in `src/icons.rs` and work offline.

IBM Plex Mono and IBM Plex Sans Condensed are embedded as subset WOFF fonts in
`web/index.html`, so the dashboard does not contact a font CDN. Their SIL Open
Font License notices are in `ibmplexmono-OFL.txt` and `ibmplexsanscondensed-OFL.txt`.

`schema-bundle.json` is a complete fallback for first launch without network
access. Its offsets, client schema and build information were fetched together
from [a2x/cs2-dumper revision 1005c258412a3657ac73ed1a9f84b9cbedddd876](https://github.com/a2x/cs2-dumper/tree/1005c258412a3657ac73ed1a9f84b9cbedddd876/output)
(build 14192). The upstream MIT notice is in `schema_LICENSE.txt`. Runtime
updates still resolve one immutable revision before fetching all three files;
unrelated loose cache files are never combined into a schema.
