local SIZE = 3400
local BASE = 760
local BASE_DAMAGE = 400

local NAMES = { "Blue", "Red", "Green", "Purple" }
local CORNERS = {
    { -1, 1 },
    { 1, -1 },
    { -1, -1 },
    { 1, 1 },
}

arena.setSize(SIZE)
teams.setCount(4)
shapes.setMax(1600)

for team, corner in ipairs(CORNERS) do
    local x = corner[1] * (SIZE - BASE * 0.5)
    local y = corner[2] * (SIZE - BASE * 0.5)
    arena.addBase(team, x, y, BASE, BASE, BASE_DAMAGE)
end

local kills = { 0, 0, 0, 0 }

local function team_name(team)
    return NAMES[team] or "Unknown"
end

events.on("player_spawn", function(event)
    local player = event.player
    if player and player.team > 0 then
        chat.send(player.id, "You are on the " .. team_name(player.team) .. " team.")
    end
end)

events.on("player_death", function(event)
    local killer = event.killer
    if killer and killer.team > 0 then
        kills[killer.team] = kills[killer.team] + 1
    end
end)

commands.register("team", function(player)
    if player.team > 0 then
        player:message("You are on the " .. team_name(player.team) .. " team.")
    else
        player:message("You are not on a team.")
    end
end)

commands.register("score", function(player)
    local parts = {}
    for team, name in ipairs(NAMES) do
        parts[#parts + 1] = name .. " " .. kills[team]
    end
    player:message("Team kills: " .. table.concat(parts, ", "))
end)

every(120, function()
    local best, best_kills = nil, -1
    for team, count in ipairs(kills) do
        if count > best_kills then
            best, best_kills = team, count
        end
    end

    if best_kills > 0 then
        chat.broadcast(team_name(best) .. " is leading with " .. best_kills .. " kills!")
    end
end)
