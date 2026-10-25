---@meta

---@class PlayerSpawnEvent
---@field id number
---@field name string
---@field player Player

---@class PlayerDisconnectEvent
---@field id number

---@class PlayerMoveEvent
---@field id number
---@field dir number

---@class PlayerAutofireEvent
---@field id number
---@field enabled boolean

---@class PlayerRepelEvent
---@field id number
---@field enabled boolean

---@class PlayerAutospinEvent
---@field id number
---@field enabled boolean

---@class PlayerAimEvent
---@field id number
---@field dir number

---@class TankSelectEvent
---@field id number
---@field tankId number
---@field player Player

---@class StatUpgradeEvent
---@field id number
---@field stat integer

---@class ChatEvent
---@field id number
---@field channel "global"|"team"
---@field text string
---@field player Player

---@class TickEvent
---@field tick integer

---@class PlayerDeathEvent
---@field id number
---@field player Player
---@field killerId? number
---@field killer? Player

---@class ShapeKillEvent
---@field id number
---@field player Player
---@field kind "square"|"triangle"|"pentagon"

---@class LevelUpEvent
---@field id number
---@field player Player
---@field level integer

---@class TankTreeEvent

---@class Events
events = {}

---@param event "player_spawn"
---@param handler fun(event: PlayerSpawnEvent)
---@overload fun(event: "player_disconnect", handler: fun(event: PlayerDisconnectEvent))
---@overload fun(event: "player_movement", handler: fun(event: PlayerMoveEvent))
---@overload fun(event: "player_autofire", handler: fun(event: PlayerAutofireEvent))
---@overload fun(event: "player_repel", handler: fun(event: PlayerRepelEvent))
---@overload fun(event: "player_autospin", handler: fun(event: PlayerAutospinEvent))
---@overload fun(event: "player_aim", handler: fun(event: PlayerAimEvent))
---@overload fun(event: "tank_select", handler: fun(event: TankSelectEvent))
---@overload fun(event: "stat_upgrade", handler: fun(event: StatUpgradeEvent))
---@overload fun(event: "chat", handler: fun(event: ChatEvent))
---@overload fun(event: "tick", handler: fun(event: TickEvent))
---@overload fun(event: "player_death", handler: fun(event: PlayerDeathEvent))
---@overload fun(event: "shape_kill", handler: fun(event: ShapeKillEvent))
---@overload fun(event: "level_up", handler: fun(event: LevelUpEvent))
---@overload fun(event: "tank_tree", handler: fun(event: TankTreeEvent))
---@overload fun(event: string, handler: fun(event: table))
function events.on(event, handler) end

---@class Commands
---@field register fun(name: string, handler: fun(player: Player, args: string[]))
commands = {}

---@class Player
---@field id integer
---@field name string
---@field x number
---@field y number
---@field health integer
---@field maxHealth integer
---@field level integer
---@field score integer
---@field team integer
---@field tank string
---@field isBot boolean
---@field alive boolean
---@field speed number
---@field barrels Barrel[]
local Player = {}

function Player:fireLaser() end

---@param x number
---@param y number
function Player:teleport(x, y) end

---@param amount? integer
function Player:heal(amount) end

function Player:kill() end

---@param text string
function Player:message(text) end

---@param amount integer
function Player:addXp(amount) end

---@param tank string
function Player:setTank(tank) end

---@param level integer
function Player:setLevel(level) end

---@param team integer
function Player:setTeam(team) end

---@class ZoneDef
---@field x number
---@field y number
---@field w number
---@field h number
---@field team? integer
---@field damage? number
---@field spawn? boolean
---@field shapes? boolean

---@class Arena
arena = {}

---@param bound number
function arena.setSize(bound) end

---@return number
function arena.size() end

---@return integer
function arena.addWall(x, y, w, h) end

---@param id integer
---@return boolean
function arena.removeWall(id) end

function arena.clearWalls() end

---@return integer
function arena.wallCount() end

---@param zone ZoneDef
---@return integer
function arena.addZone(zone) end

---@param team integer
---@param damage? number
---@return integer
function arena.addBase(team, x, y, w, h, damage) end

---@param id integer
---@return boolean
function arena.removeZone(id) end

function arena.clearZones() end

---@param radius? number
---@return boolean
function arena.isBlocked(x, y, radius) end

---@class Teams
teams = {}

---@param count integer
function teams.setCount(count) end

---@return integer
function teams.count() end

---@class Players
players = {}

---@param id integer
---@return Player?
function players.get(id) end

---@return Player[]
function players.all() end

---@return integer
function players.count() end

---@class Chat
chat = {}

---@param text string
function chat.broadcast(text) end

---@param id integer
---@param text string
function chat.send(id, text) end

---@class Shapes
shapes = {}

---@param max? integer
function shapes.setMax(max) end

---@class Game
---@field tickRate number
game = {}

---@return integer
function game.tick() end

---@class Task
task = {}

---@param f fun()
function task.spawn(f) end

---@param seconds number
function wait(seconds) end

---@param seconds number
---@param f fun()
function every(seconds, f) end

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
local Barrel = {}

---@param params BulletParams
function Barrel:spawnBullet(params) end

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
