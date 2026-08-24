use mlua::UserData;

use crate::entities::entity::EntityId;

#[derive(Debug, Clone)]
pub struct LuaPlayer(EntityId);

#[allow(clippy::extra_unused_lifetimes)]
impl<'a, 'b> UserData for LuaPlayer {
    fn add_fields<F: mlua::prelude::LuaUserDataFields<Self>>(fields: &mut F) {
        // fields.add_field_method_get("speed", |lua, this: &LuaPlayer| {
        //     let entities = lua.app_data_ref::<Entities>().unwrap();
        //     Ok(entities.speed_of(this.0))
        // });
        // fields.add_field_method_set("speed", |lua, this: &mut LuaPlayer, v:
        // f32| {     let entities =
        // lua.app_data_mut::<Entities>().unwrap();     entities.
        // set_speed(this.0, v);     Ok(())
        // });
    }

    fn add_methods<M: mlua::prelude::LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method("spawnBullet", |_, this, ()| Ok(()));
    }
}
