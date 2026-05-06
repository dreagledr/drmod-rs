# MGR Plugin SDK Analysis

## Overview

The `mgr-plugin-sdk` is a C++ SDK for creating plugins/mods for Metal Gear Rising: Revengeance. It contains 529 header files with reverse-engineered game structures, classes, and addresses.

## Key Static Addresses

| Address | What | Header |
|---------|------|--------|
| `base + 0x17EA100` | `PlayerManagerImplement*` — player manager singleton | `PlayerManagerImplement.h` |
| `base + 0x1737A10` | `cPlayerInfoManager&` — UI/info manager | `cPlayerInfoManager.h` |
| `base + 0x17E9C2C` | `BehaviorWeapon::ms_Context` — weapon context | `BehaviorWeapon.h` |
| `base + 0x17E9DBC` | `cPl0000Weapon::ms_Context` — player weapon context | `cPl0000Weapon.h` |
| `base + 0x19C1430` | `cGameUIManager&` — game UI manager singleton | `cGameUIManager.h` |
| `base + 0x17E9F9C` | `GameMenuStatus` — game/pause/menu state (enum 0-18) | `shared.h` (from C++ project) |

## PlayerManagerImplement (size 0x100)

Main player state manager. Key fields:

| Offset | Field | Type | Description |
|--------|-------|------|-------------|
| 0xC4 | `m_nMaxHealth` | int | Max HP |
| 0xC8 | `m_fMaxFuelContainer` | float | Max MP |
| 0xCC | `m_nHealthBonus` | int | HP upgrades count |
| 0xD0 | `m_nFuelContainerBonus` | int | MP upgrades count |
| 0xD4 | `m_nBattlePoints` | int | BP count |
| 0xE0 | `m_nMainWeaponEquipped` | int | Main weapon type ID |
| 0xE4 | `m_nCustomWeaponEquipped` | int | Custom weapon type ID |
| 0xE8 | `m_nSubWeaponEquipped` | int | Sub weapon type ID |
| 0xEC | `m_nRecoveryEquipped` | int | Recovery item type ID |

> **Note:** SDK headers list offsets 0xD8/0xDC/0xE0, but runtime testing shows actual offsets are 0xE0/0xE4/0xE8. The struct in game binary has 8 extra bytes of padding before weapon fields.

VMT methods: `getMainWeaponEquipped()`, `getSubWeaponEquipped()`, `getCustomWeaponEquipped()`, `getBP()`, `getHealthUpgrades()`, `getFuelContainerUpgrades()`, `isPlayerAlive()`, `upgradeHealth()`, `addBP()`, etc.

## Pl0000 (Player Entity, ~20KB struct)

The player entity class (`Pl0000.h`, 3214 lines). Key fields:

- `m_vecTransPos` — position vector
- `m_vecVelocity` — velocity vector
- `m_nHealth` — current HP
- `m_fNinjaRunSpeedRate`, `m_fWallRunSpeedRate`
- `m_bRipperModeEnabled` — Ripper Mode state
- `m_nBladeModeType` — Blade Mode type
- `m_SwordState` — sword state
- `m_nButtonJump`, `m_nButtonLightAttack`, `m_nButtonHeavyAttack`, `m_nButtonBlademode`, `m_nButtonNinjarun`
- `m_CustomWeaponHandle`, `m_SwordHandle`, `m_SheathHandle` — entity handles
- Methods: `isBladeModeActive()`, `isInAir()`, `isOnGround()`, `isRunning()`, `isIdle()`, `canActivateRipperMode()`, `enableRipperMode()`, `disableRipperMode()`, `getMaxHealth()`, `getFuelContainer()`, etc.

## cGameUIManager (size 0xDC)

UI manager accessible from examples:

- `m_pPlayerEntity` — player Entity pointer
- `m_pPlayer` — Pl1500* (player character)
- `m_vecPlayerPosition` — player position
- `m_pWeaponInfoDispParts` — weapon info display

## Weapon System

- **`BehaviorWeapon`** (size 0x8C0) — base weapon behavior class
  - `WeaponData` struct (size 0x90) — weapon data
- **`cPl0000Weapon`** (size 0x980) — player weapon class
- **`cPl0000SaiWeapon`** (size 0xDE0) — sai weapon subclass
- **`Em0110Weapon`** — enemy weapon class with `m_WeaponData[5]` array

Weapon types are passed as raw `int` values. **No enum or constants exist** in the SDK mapping weapon names to IDs. Weapon type IDs must be discovered through runtime experimentation or Cheat Engine.

## Key Addresses for Our Rust Mod

For reading from Rust (current mod):

```
base + 0x177B4A4  → Player object pointer (already used)
  +0x50 = X pos (f32)
  +0x54 = Y pos (f32)
  +0x58 = Z pos (f32)
  +0x870 = Current HP (i32)

base + 0x17EA100  → PlayerManagerImplement pointer
  +0xD8 = m_nCustomWeaponEquipped (int)
  +0xDC = m_nSubWeaponEquipped (int)

base + 0x17E9F9C  → GameMenuStatus (int, enum 0-18)
```

## Cheat Engine Script (Enemy Step)

From `.CT` file — "create a platform below you while holding L3":
- Injection point: `mov [eax],00000000` at `E6B45E`
- Input address: `base + 0x19D05F4`, value `0x40` = L3 pressed
- Ground states: `0` = airborne, `1` = landing, `2` = grounded
- NOPs `mov [esi+10],eax` at `4E98CD` to prevent overwriting

## SDK Usage Pattern (from examples)

All examples use event-based pattern:
```cpp
class Plugin {
    Plugin() {
        Events::OnTickEvent += []() { /* game logic each frame */ };
    }
} __plugin;
```

Player access: `cGameUIManager::ms_Instance.m_pPlayer` or `PlayerManagerImplement::get()`
