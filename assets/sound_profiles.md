Sound radius model
==================

The rings describe the local player's emitted sounds. They are upper estimates
in open space, not a claim that another player can hear through walls or floors.
Source coordinates are converted with 0.0254 metres per world unit.

The reference is Valve's public audio data mirrored by SteamDatabase,
revision `b73cfdeb35e48bb20a5e6b2edc8ee3aad14dcf39`:

- [game_sounds_footsteps.vsndevts](https://github.com/SteamDatabase/GameTracking-CS2/blob/b73cfdeb35e48bb20a5e6b2edc8ee3aad14dcf39/game/csgo/pak01_dir/soundevents/game_sounds_footsteps.vsndevts):
  `Base.Footstep` and `Base.Land` reach zero on the distance/volume curve at 1100
  units (27.94 m). Surface overrides share this endpoint. Actual audibility can
  be lower because of volume, materials, occlusion and sound settings.
- [game_sounds_weapons.vsndevts](https://github.com/SteamDatabase/GameTracking-CS2/blob/b73cfdeb35e48bb20a5e6b2edc8ee3aad14dcf39/game/csgo/pak01_dir/soundevents/game_sounds_weapons.vsndevts):
  `Weapon_USP.SilencedShot`, `Weapon_M4A1.Silenced` and `Weapon_MP5.Single` reach
  zero at 1400 units (35.56 m). USP/M4 detection uses the actual silencer state.
  The unsuppressed USP has no distant child and reaches zero at 2500 units.
  `Weapon_Taser.Single` reaches zero at 2420.285645 units. Ordinary unsuppressed
  weapons use distant child events; the near curve's endpoint is not a cutoff.
  Their 9000-unit broadcast limit is displayed with ≤ as an upper bound.
- [game_sounds_player.vsndevts](https://github.com/SteamDatabase/GameTracking-CS2/blob/b73cfdeb35e48bb20a5e6b2edc8ee3aad14dcf39/game/csgo/pak01_dir/soundevents/game_sounds_player.vsndevts):
  `Default.WalkJump` has a 98-unit broadcast override, below its 493.714294-unit
  distance-curve endpoint. A jump ring requires an emitted sound and an upward
  ground-to-air transition; changing ground flags alone does not emit sound.

Footsteps use movement-service step-side changes, emitted-sound timestamps or
step-timer resets. Shift walking, crouching, airborne and unreadable ground
states do not become running sounds. If game signals are unavailable, an
explicit estimate requires both travelled distance and elapsed time. Weapon
switches are not automatically treated as audible events. Each impulse stores
its emission position, sequence ID and age; rendering never moves an old sound
along with the player's current position. The pulse reaches the model radius
and fades out instead of displaying a persistent filled disk.

The profile is bundled for offline use. Recheck these values when Valve changes
the audio data; server overrides and spatial sound propagation remain outside
the model.
