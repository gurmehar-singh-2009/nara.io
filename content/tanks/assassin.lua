-- Assassin (converted from tank_defs.json)
-- units are game units (tank body = 42), angles are degrees.
return {
    id = 15,
    name = "Assassin",
    upgradeMessage = "",
    levelRequirement = 30,
    upgrades = {
        "ranger",
        "stalker",
    },
    speed = 1,
    maxHealth = 50,
    sides = 1,
    fieldFactor = 0.8,
    absorbtionFactor = 1,
    preAddon = 0,
    postAddon = 0,
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
        {
            x = 0,
            y = 0,
            angle = 0,
            width = 17.64,
            length = 50.4,
            delay = 0,
            reload = 2,
            recoil = 3,
            isTrapezoid = false,
            trapezoidDirection = 0,
            addon = 0,
            bullet = {
                type = "bullet",
                health = 1,
                damage = 1,
                speed = 1.5,
                scatterRate = 1,
                lifeLength = 1,
                absorbtionFactor = 1,
                sizeRatio = 1,
            },
        },
    },
    onShoot = function(player)
        -- TODO
    end,
}
