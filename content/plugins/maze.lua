local SIZE = 3200
local CELL = 170
local LOOP_CHANCE = 0.18
local OPEN_CENTER = 2

arena.setSize(SIZE)
teams.setCount(0)
shapes.setMax(900)

local n = math.floor(SIZE * 2 / CELL)
if n % 2 == 0 then
    n = n - 1
end

local cell = SIZE * 2 / n
local grid = {}

for y = 0, n - 1 do
    grid[y] = {}
    for x = 0, n - 1 do
        grid[y][x] = true
    end
end

local function shuffle(t)
    for i = #t, 2, -1 do
        local j = math.random(i)
        t[i], t[j] = t[j], t[i]
    end
end

local stack = { { 1, 1 } }
grid[1][1] = false

while #stack > 0 do
    local top = stack[#stack]
    local x, y = top[1], top[2]
    local dirs = { { 2, 0 }, { -2, 0 }, { 0, 2 }, { 0, -2 } }
    shuffle(dirs)

    local moved = false
    for _, d in ipairs(dirs) do
        local nx, ny = x + d[1], y + d[2]
        if nx > 0 and ny > 0 and nx < n - 1 and ny < n - 1 and grid[ny][nx] then
            grid[y + d[2] // 2][x + d[1] // 2] = false
            grid[ny][nx] = false
            stack[#stack + 1] = { nx, ny }
            moved = true
            break
        end
    end

    if not moved then
        stack[#stack] = nil
    end
end

for y = 1, n - 2 do
    for x = 1, n - 2 do
        if grid[y][x] and math.random() < LOOP_CHANCE then
            local horizontal = not grid[y][x - 1] and not grid[y][x + 1]
            local vertical = not grid[y - 1][x] and not grid[y + 1][x]
            if horizontal ~= vertical then
                grid[y][x] = false
            end
        end
    end
end

local mid = n // 2
for y = mid - OPEN_CENTER, mid + OPEN_CENTER do
    for x = mid - OPEN_CENTER, mid + OPEN_CENTER do
        grid[y][x] = false
    end
end

local open = {}
local done = {}

local function close_rect(r)
    local x = -SIZE + (r.x0 + r.x1 + 1) * 0.5 * cell
    local y = -SIZE + (r.y0 + r.y1 + 1) * 0.5 * cell
    local w = (r.x1 - r.x0 + 1) * cell
    local h = (r.y1 - r.y0 + 1) * cell
    done[#done + 1] = { x, y, w, h }
end

for y = 0, n - 1 do
    local next_open = {}
    local x = 0

    while x < n do
        if grid[y][x] then
            local x0 = x
            while x + 1 < n and grid[y][x + 1] do
                x = x + 1
            end

            local key = x0 .. ":" .. x
            local r = open[key]
            if r then
                r.y1 = y
                open[key] = nil
            else
                r = { x0 = x0, x1 = x, y0 = y, y1 = y }
            end
            next_open[key] = r
        end
        x = x + 1
    end

    for _, r in pairs(open) do
        close_rect(r)
    end
    open = next_open
end

for _, r in pairs(open) do
    close_rect(r)
end

for _, w in ipairs(done) do
    arena.addWall(w[1], w[2], w[3], w[4])
end

print("maze: " .. n .. "x" .. n .. " grid, " .. arena.wallCount() .. " walls")

events.on("player_spawn", function(event)
    wait(1)
    chat.send(event.id, "Welcome to the maze. Watch the corners!")
end)
