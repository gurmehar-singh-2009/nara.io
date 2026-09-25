-- Auto 3 (converted from tank_defs.json)
-- units are game units (tank body = 42), angles are degrees.
return {
    id = 41,
    name = "Auto 3",
    upgradeMessage = "",
    levelRequirement = 30,
    upgrades = {
        "auto_5",
        "auto_gunner",
    },
    speed = 1,
    maxHealth = 50,
    sides = 1,
    fieldFactor = 1,
    absorbtionFactor = 1,
    preAddon = 0,
    postAddon = 84,
    flags = {
        invisibility = false,
        zoomAbility = false,
        canShoot = true,
        devOnly = false,
    },
    stats = {
        { name = "Movement Speed", max = 7 },
        { name = "Reload", max = 7 },
        { name = "Bullet Damage", max = 7 },
        { name = "Bullet Penetration", max = 7 },
        { name = "Bullet Speed", max = 7 },
        { name = "Body Damage", max = 7 },
        { name = "Max Health", max = 7 },
        { name = "Health Regen", max = 7 },
    },
    barrels = {
    },
    onShoot = function(player)
        -- TODO
    end,
}
