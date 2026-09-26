events.on("player_spawn", function (event)
    print("player joined" .. event.name)

    wait(2)

    print("works!!")
end)

events.on("player_autofire", function(event)
    print(event.id .. " " .. tostring(event.enabled))
end)
