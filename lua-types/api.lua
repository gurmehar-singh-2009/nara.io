---@meta

---@class PlayerSpawnEvent
---@field id number
---@field name string

---@class PlayerDisconnectEvent
---@field id number

---@class PlayerMoveEvent
---@field id number
---@field dir number

---@class PlayerAutofireEvent
---@field id number
---@field enabled boolean

---@class PlayerAimEvent
---@field id number
---@field dir number

---@class TankSelectEvent
---@field id number
---@field tankId number

---@class TankTreeEvent

---@class Events
events = {}

---@param event "player_spawn"
---@param handler fun(event: PlayerSpawnEvent)
---@overload fun(event: string, handler: fun(event: table))

---@param event "player_disconnect"
---@param handler fun(event: PlayerDisconnectEvent)
---@overload fun(event: string, handler: fun(event: table))

function events.on(event, handler) end

---@class Commands
---@field register fun(name: string, handler: fun(player: Player))
commands = {}

---@class BulletParams
---@field damage number
---@field penetration number
---@field speed number
---@field endurance number     -- seconds alive before it despawns
---@field size? number         -- radius; defaults from barrel width if omitted
---@field knockback? number    -- push force applied to whatever it hits
---@field spread? number       -- degrees of random angle deviation per shot
---@field onHit? fun(target: Player) -- custom effect when this bullet lands

---@class Barrel
---@field x number
---@field y number
---@field angle number
---@field width number
---@field length number
---@param params BulletParams
function Barrel:spawnBullet(params) end

---@class Player
---@field speed number
---@field health integer
---@field barrels Barrel[]
function Player:fireLaser() end

---@class BarrelDef
---@field x number
---@field y number
---@field angle number
---@field width number
---@field length number

---@class TankDef
---@field name string
---@field health integer
---@field speed number
---@field barrels BarrelDef[]
---@field onShoot fun(player: Player)

---@class WeaponDef
---@field reload number
---@field onShoot fun(player: Player)

---@param seconds number
function wait(seconds) end
