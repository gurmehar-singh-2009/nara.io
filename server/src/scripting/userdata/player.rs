use glam::Vec2;
use mlua::{Lua, UserData, UserDataFields, UserDataMethods, Value};

use crate::{
    entities::entity::{Entities, EntityId},
    scripting::api::{
        LuaAction, bridge, entities, entity_of, lua_team, push_action, server_team, value_or_nil,
    },
};

#[derive(Debug, Clone, Copy)]
pub struct LuaPlayer(pub u32);

fn read<R>(
    lua: &Lua,
    conn: u32,
    f: impl FnOnce(&Entities, EntityId) -> Option<R>,
) -> mlua::Result<Option<R>> {
    let Some(eid) = entity_of(lua, conn)? else {
        return Ok(None);
    };

    let entities = entities(lua)?;
    let entities = entities
        .lock()
        .map_err(|e| mlua::Error::external(e.to_string()))?;

    if !entities.is_alive(eid) {
        return Ok(None);
    }

    Ok(f(&entities, eid))
}

fn write(lua: &Lua, conn: u32, f: impl FnOnce(&mut Entities, EntityId)) -> mlua::Result<()> {
    let Some(eid) = entity_of(lua, conn)? else {
        return Ok(());
    };

    let entities = entities(lua)?;
    let mut entities = entities
        .lock()
        .map_err(|e| mlua::Error::external(e.to_string()))?;

    if entities.is_alive(eid) {
        f(&mut entities, eid);
    }

    Ok(())
}

fn field<R: mlua::IntoLua>(
    lua: &Lua,
    conn: u32,
    f: impl FnOnce(&Entities, EntityId) -> Option<R>,
) -> mlua::Result<Value> {
    let value = read(lua, conn, f)?;
    value_or_nil(lua, value)
}

impl UserData for LuaPlayer {
    fn add_fields<F: UserDataFields<Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, this| Ok(this.0));

        fields.add_field_method_get("name", |lua, this| {
            field(lua, this.0, |e, id| {
                e.tanks.get(id).map(|t| t.name.to_string())
            })
        });

        fields.add_field_method_get("x", |lua, this| {
            field(lua, this.0, |e, id| Some(e.positions[id.index].x))
        });

        fields.add_field_method_set("x", |lua, this, x: f32| {
            write(lua, this.0, |e, id| e.positions[id.index].x = x)
        });

        fields.add_field_method_get("y", |lua, this| {
            field(lua, this.0, |e, id| Some(e.positions[id.index].y))
        });

        fields.add_field_method_set("y", |lua, this, y: f32| {
            write(lua, this.0, |e, id| e.positions[id.index].y = y)
        });

        fields.add_field_method_get("health", |lua, this| {
            field(lua, this.0, |e, id| e.get(id).map(|r| *r.health))
        });

        fields.add_field_method_set("health", |lua, this, health: u32| {
            write(lua, this.0, |e, id| {
                let max = e.tanks.get(id).map(|t| *t.max_health).unwrap_or(health);
                if let Some(h) = e.health_mut(id) {
                    *h = health.clamp(1, max.max(1));
                }
            })
        });

        fields.add_field_method_get("maxHealth", |lua, this| {
            field(lua, this.0, |e, id| e.tanks.get(id).map(|t| *t.max_health))
        });

        fields.add_field_method_get("level", |lua, this| {
            field(lua, this.0, |e, id| e.tanks.get(id).map(|t| *t.level))
        });

        fields.add_field_method_set("level", |lua, this, level: u32| {
            push_action(lua, LuaAction::SetLevel(this.0, level))
        });

        fields.add_field_method_get("score", |lua, this| {
            field(lua, this.0, |e, id| e.tanks.get(id).map(|t| *t.score))
        });

        fields.add_field_method_get("team", |lua, this| {
            field(lua, this.0, |e, id| {
                e.tanks.get(id).map(|t| lua_team(*t.team))
            })
        });

        fields.add_field_method_set("team", |lua, this, team: u8| {
            push_action(lua, LuaAction::SetTeam(this.0, server_team(team)))
        });

        fields.add_field_method_get("tank", |lua, this| {
            field(lua, this.0, |e, id| {
                e.tanks.get(id).map(|t| t.tank_type.name.clone())
            })
        });

        fields.add_field_method_set("tank", |lua, this, tank: String| {
            push_action(lua, LuaAction::SetTank(this.0, tank))
        });

        fields.add_field_method_get("isBot", |lua, this| {
            Ok(this.0
                >= bridge(lua)?
                    .lock()
                    .map(|b| b.bot_id_base)
                    .unwrap_or(u32::MAX))
        });

        fields.add_field_method_get("alive", |lua, this| {
            Ok(read(lua, this.0, |_, _| Some(()))?.is_some())
        });
    }

    fn add_methods<M: UserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("teleport", |lua, this, (x, y): (f32, f32)| {
            write(lua, this.0, |e, id| {
                e.positions[id.index] = Vec2::new(x, y);
                e.velocities[id.index] = Vec2::ZERO;
                e.recoil_velocities[id.index] = Vec2::ZERO;
            })
        });

        methods.add_method("heal", |lua, this, amount: Option<u32>| {
            write(lua, this.0, |e, id| {
                let max = e.tanks.get(id).map(|t| *t.max_health).unwrap_or(0);
                if let Some(h) = e.health_mut(id) {
                    *h = amount.map_or(max, |a| h.saturating_add(a)).min(max);
                }
            })
        });

        methods.add_method("kill", |lua, this, ()| {
            push_action(lua, LuaAction::Kill(this.0))
        });

        methods.add_method("message", |lua, this, text: String| {
            push_action(lua, LuaAction::Message(this.0, text))
        });

        methods.add_method("addXp", |lua, this, amount: u32| {
            push_action(lua, LuaAction::AddXp(this.0, amount))
        });

        methods.add_method("setTank", |lua, this, tank: String| {
            push_action(lua, LuaAction::SetTank(this.0, tank))
        });

        methods.add_method("setLevel", |lua, this, level: u32| {
            push_action(lua, LuaAction::SetLevel(this.0, level))
        });

        methods.add_method("setTeam", |lua, this, team: u8| {
            push_action(lua, LuaAction::SetTeam(this.0, server_team(team)))
        });

        methods.add_meta_method(mlua::MetaMethod::Eq, |_, this, other: mlua::AnyUserData| {
            Ok(other
                .borrow::<LuaPlayer>()
                .map(|o| o.0 == this.0)
                .unwrap_or(false))
        });

        methods.add_meta_method(mlua::MetaMethod::ToString, |_, this, ()| {
            Ok(format!("Player({})", this.0))
        });
    }
}
