-- Auto Smasher (converted from tank_defs.json)
-- units are game units (tank body = 42), angles are degrees.
return {
    id = 50,
    name = "Auto Smasher",
    upgradeMessage = "",
    levelRequirement = 45,
    upgrades = {},
    speed = 1,
    maxHealth = 50,
    sides = 1,
    fieldFactor = 0.899,
    absorbtionFactor = 1,
    preAddon = 0,
    postAddon = 91,
    flags = {
        invisibility = false,
        zoomAbility = false,
        canShoot = true,
        devOnly = false,
    },
    stats = {
        { name = "Movement Speed", max = 10 },
        { name = "Reload", max = 10 },
        { name = "Bullet Damage", max = 10 },
        { name = "Bullet Penetration", max = 10 },
        { name = "Bullet Speed", max = 10 },
        { name = "Body Damage", max = 10 },
        { name = "Max Health", max = 10 },
        { name = "Health Regen", max = 10 },
    },
    barrels = {
    },
    onShoot = function(player)
        -- TODO
    end,
}
