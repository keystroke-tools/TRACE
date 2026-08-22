# Assetto Corsa shared-memory API reference

This is TRACE's working reference for the original Assetto Corsa (AC1) vanilla
shared-memory API. It records the names and layouts we need during development so
contributors do not have to reconstruct the SDK structs from memory.

Primary published reference: [Assetto Corsa Japan LABS shared-memory reference](https://labs.assettocorsa.jp/documents/reference/shared_memory).
The API uses little-endian values, Windows two-byte `wchar_t`, and 4-byte structure
packing. Array indexes use AC order: front-left, front-right, rear-left, rear-right.

## Pages and mapping names

| Windows mapping | C/C++ structure | Bytes | Typical cadence | Purpose |
|---|---|---:|---|---|
| `acpmf_physics` | `SPageFilePhysics` | 580 | physics update | vehicle dynamics and controls |
| `acpmf_graphics` | `SPageFileGraphic` | 296 | rendered frame | session, lap, flag, pit, and HUD state |
| `acpmf_static` | `SPageFileStatic` | 684 | session load | car, track, assists, and session configuration |

TRACE opens existing mappings read-only. Physics and graphics snapshots are accepted
only when `packetId` is stable before, during, and after the copy. The static page has
no packet counter and is copied with the changing pages.

## Enumerations

| Type | Values |
|---|---|
| `AC_STATUS` | `0 OFF`, `1 REPLAY`, `2 LIVE`, `3 PAUSE` |
| `AC_SESSION_TYPE` | `-1 UNKNOWN`, `0 PRACTICE`, `1 QUALIFY`, `2 RACE`, `3 HOTLAP`, `4 TIME_ATTACK`, `5 DRIFT`, `6 DRAG` |
| `AC_FLAG_TYPE` | `0 NONE`, `1 BLUE`, `2 YELLOW`, `3 BLACK`, `4 WHITE`, `5 CHECKERED`, `6 PENALTY` |

## Physics page — `acpmf_physics`

| Offset | Source field | Type | Published meaning/unit |
|---:|---|---|---|
| 0 | `packetId` | `int` | update counter |
| 4 | `gas` | `float` | throttle, 0–1 |
| 8 | `brake` | `float` | brake, 0–1 |
| 12 | `fuel` | `float` | litres |
| 16 | `gear` | `int` | 0 reverse, 1 neutral, 2+ forward |
| 20 | `rpms` | `int` | RPM |
| 24 | `steerAngle` | `float` | signed steering input ratio; actual wheel/road-wheel angle requires car and controller configuration |
| 28 | `speedKmh` | `float` | km/h |
| 32 | `velocity[3]` | `float[3]` | world velocity |
| 44 | `accG[3]` | `float[3]` | local acceleration in g |
| 56 | `wheelSlip[4]` | `float[4]` | wheel slip |
| 72 | `wheelLoad[4]` | `float[4]` | wheel load, N |
| 88 | `wheelsPressure[4]` | `float[4]` | tyre pressure; source unit not asserted by TRACE |
| 104 | `wheelAngularSpeed[4]` | `float[4]` | wheel angular speed |
| 120 | `tyreWear[4]` | `float[4]` | tyre wear |
| 136 | `tyreDirtyLevel[4]` | `float[4]` | tyre dirt level |
| 152 | `tyreCoreTemperature[4]` | `float[4]` | °C |
| 168 | `camberRAD[4]` | `float[4]` | radians |
| 184 | `suspensionTravel[4]` | `float[4]` | metres |
| 200 | `drs` | `float` | DRS state, 0/1 |
| 204 | `tc` | `float` | TC slip-ratio limit |
| 208 | `heading` | `float` | world heading |
| 212 | `pitch` | `float` | world pitch |
| 216 | `roll` | `float` | world roll |
| 220 | `cgHeight` | `float` | centre-of-gravity height |
| 224 | `carDamage[5]` | `float[5]` | body damage channels |
| 244 | `numberOfTyresOut` | `int` | tyres outside track limits |
| 248 | `pitLimiterOn` | `int` | pit limiter state |
| 252 | `abs` | `float` | ABS slip-ratio limit |
| 256 | `kersCharge` | `float` | KERS/ERS charge ratio |
| 260 | `kersInput` | `float` | KERS/ERS input |
| 264 | `autoShifterOn` | `int` | automatic shifter state |
| 268 | `rideHeight[2]` | `float[2]` | front/rear ride height |
| 276 | `turboBoost` | `float` | turbo boost |
| 280 | `ballast` | `float` | kg |
| 284 | `airDensity` | `float` | air density |
| 288 | `airTemp` | `float` | °C |
| 292 | `roadTemp` | `float` | °C |
| 296 | `localAngularVel[3]` | `float[3]` | local angular velocity |
| 308 | `finalFF` | `float` | final force-feedback value |
| 312 | `performanceMeter` | `float` | delta to best performance |
| 316 | `engineBrake` | `int` | engine-brake setting |
| 320 | `ersRecoveryLevel` | `int` | ERS recovery setting |
| 324 | `ersPowerLevel` | `int` | ERS power setting |
| 328 | `ersHeatCharging` | `int` | ERS heat/battery charge state |
| 332 | `ersIsCharging` | `int` | ERS charging state |
| 336 | `kersCurrentKJ` | `float` | lap KERS/ERS usage, kJ |
| 340 | `drsAvailable` | `int` | DRS available |
| 344 | `drsEnabled` | `int` | DRS enabled |
| 348 | `brakeTemp[4]` | `float[4]` | brake temperature, °C |
| 364 | `clutch` | `float` | clutch, 0–1 |
| 368 | `tyreTempI[4]` | `float[4]` | inner tyre temperature, °C |
| 384 | `tyreTempM[4]` | `float[4]` | middle tyre temperature, °C |
| 400 | `tyreTempO[4]` | `float[4]` | outer tyre temperature, °C |
| 416 | `isAIControlled` | `int` | AI-control state |
| 420 | `tyreContactPoint[4][3]` | `float[12]` | contact points |
| 468 | `tyreContactNormal[4][3]` | `float[12]` | contact normals |
| 516 | `tyreContactHeading[4][3]` | `float[12]` | contact headings |
| 564 | `brakeBias` | `float` | front bias, 0–1 |
| 568 | `localVelocity[3]` | `float[3]` | vehicle-local velocity |

## Graphics page — `acpmf_graphics`

| Offset | Source field | Type | Published meaning/unit |
|---:|---|---|---|
| 0 | `packetId` | `int` | update counter |
| 4 | `status` | `AC_STATUS` | simulator state |
| 8 | `session` | `AC_SESSION_TYPE` | session type |
| 12 | `currentTime[15]` | `wchar_t[15]` | formatted current lap time |
| 42 | `lastTime[15]` | `wchar_t[15]` | formatted last lap time |
| 72 | `bestTime[15]` | `wchar_t[15]` | formatted best lap time |
| 102 | `split[15]` | `wchar_t[15]` | formatted split |
| 132 | `completedLaps` | `int` | completed lap count |
| 136 | `position` | `int` | race position |
| 140 | `iCurrentTime` | `int` | current lap, ms |
| 144 | `iLastTime` | `int` | last lap, ms |
| 148 | `iBestTime` | `int` | best lap, ms |
| 152 | `sessionTimeLeft` | `float` | seconds |
| 156 | `distanceTraveled` | `float` | distance travelled |
| 160 | `isInPit` | `int` | in pit box/area |
| 164 | `currentSectorIndex` | `int` | zero-based current sector |
| 168 | `lastSectorTime` | `int` | completed sector, ms |
| 172 | `numberOfLaps` | `int` | configured session laps |
| 176 | `tyreCompound[33]` | `wchar_t[33]` | compound name |
| 244 | `replayTimeMultiplier` | `float` | replay speed multiplier |
| 248 | `normalizedCarPosition` | `float` | spline position, 0–1 |
| 252 | `carCoordinates[3]` | `float[3]` | world position |
| 264 | `penaltyTime` | `float` | seconds |
| 268 | `flag` | `AC_FLAG_TYPE` | active flag |
| 272 | `idealLineOn` | `int` | ideal-line aid state |
| 276 | `isInPitLane` | `int` | in pit lane |
| 280 | `surfaceGrip` | `float` | surface-grip value |
| 284 | `mandatoryPitDone` | `int` | mandatory stop complete |
| 288 | `windSpeed` | `float` | wind speed |
| 292 | `windDirection` | `float` | degrees, 0–359 |

## Static page — `acpmf_static`

| Offset | Source field | Type | Published meaning/unit |
|---:|---|---|---|
| 0 | `smVersion[15]` | `wchar_t[15]` | shared-memory version |
| 30 | `acVersion[15]` | `wchar_t[15]` | simulator version |
| 60 | `numberOfSessions` | `int` | session count |
| 64 | `numCars` | `int` | maximum cars |
| 68 | `carModel[33]` | `wchar_t[33]` | player car ID |
| 134 | `track[33]` | `wchar_t[33]` | track ID |
| 200 | `playerName[33]` | `wchar_t[33]` | given name |
| 266 | `playerSurname[33]` | `wchar_t[33]` | surname |
| 332 | `playerNick[33]` | `wchar_t[33]` | nickname |
| 400 | `sectorCount` | `int` | sectors |
| 404 | `maxTorque` | `float` | maximum torque |
| 408 | `maxPower` | `float` | maximum power |
| 412 | `maxRpm` | `int` | maximum RPM |
| 416 | `maxFuel` | `float` | litres |
| 420 | `suspensionMaxTravel[4]` | `float[4]` | maximum suspension travel |
| 436 | `tyreRadius[4]` | `float[4]` | tyre radius |
| 452 | `maxTurboBoost` | `float` | maximum boost |
| 456 | `deprecated_1` | `float` | deprecated slot; do not assign meaning |
| 460 | `deprecated_2` | `float` | deprecated slot; do not assign meaning |
| 464 | `penaltiesEnabled` | `int` | cut penalties enabled |
| 468 | `aidFuelRate` | `float` | fuel-consumption multiplier |
| 472 | `aidTireRate` | `float` | tyre-wear multiplier |
| 476 | `aidMechanicalDamage` | `float` | mechanical-damage multiplier |
| 480 | `aidAllowTyreBlankets` | `int` | tyre blankets allowed |
| 484 | `aidStability` | `float` | stability assistance |
| 488 | `aidAutoClutch` | `int` | automatic clutch |
| 492 | `aidAutoBlip` | `int` | automatic throttle blip |
| 496 | `hasDRS` | `int` | car has DRS |
| 500 | `hasERS` | `int` | car has ERS |
| 504 | `hasKERS` | `int` | car has KERS |
| 508 | `kersMaxJ` | `float` | maximum KERS energy, J |
| 512 | `engineBrakeSettingsCount` | `int` | engine-brake setting count |
| 516 | `ersPowerControllerCount` | `int` | ERS controller setting count |
| 520 | `trackSPlineLength` | `float` | spline length, m |
| 524 | `trackConfiguration[33]` | `wchar_t[33]` | layout ID |
| 592 | `ersMaxJ` | `float` | maximum ERS energy, J |
| 596 | `isTimedRace` | `int` | timed-race state |
| 600 | `hasExtraLap` | `int` | extra lap after timed race |
| 604 | `carSkin[33]` | `wchar_t[33]` | skin ID |
| 672 | `reversedGridPositions` | `int` | reversed-grid car count |
| 676 | `PitWindowStart` | `int` | pit-window start |
| 680 | `PitWindowEnd` | `int` | pit-window end |

## TRACE storage mapping

Arrow schema v4 keeps the portable canonical columns and adds:

| Column | Arrow type | Content |
|---|---|---|
| `native_schema` | UTF-8 | `assetto-corsa.shared-memory/1` |
| `native_payload` | binary | exact `ACSM` envelope containing all three pages |
| `native_float_fields` | map UTF-8 → float64 | every documented source float |
| `native_integer_fields` | map UTF-8 → int64 | every documented source integer/enum |
| `native_text_fields` | map UTF-8 → UTF-8 | every documented source string |

Keys are lower snake case and page-qualified, such as `physics.brake_temperature_c.0`,
`graphics.current_sector_index`, or `static.track_configuration`. Array indexes are
zero-based. These map columns are deliberately stable: adding another source field
adds a key, not another top-level Arrow schema version. The opaque page envelope is
also retained so a later decoder can recover fields that were unknown when recorded.

TRACE promotes `physics.number_of_tyres_out` into a per-lap maximum in SQLite for
quick archive display. The UI warns only for a maximum of three or four; one or two
tyres outside are retained without being presented as an invalidation. This is
track-limit evidence, not a validity verdict: the
published pages contain no final lap-valid flag or invalidation threshold. Penalty
time, penalty flags, and `penaltiesEnabled` remain available in the native maps for
future analysis without another Arrow schema revision.

The static page contains player names. Local recordings and exported Arrow files must
therefore be treated as potentially identifying. Checked-in fixtures use TRACE's
redaction path and contain only version, car, track, and validated condition values.
