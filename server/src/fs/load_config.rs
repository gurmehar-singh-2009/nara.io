#![allow(dead_code)]

use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use snafu::prelude::*;

macro_rules! config_sections {
    ($(
        $(#[$section_meta:meta])*
        $section:ident {
            $(
                $(#[$field_meta:meta])*
                $field:ident : $ty:ty
            ),+ $(,)?
        }
    ),+ $(,)?) => {
        $(
            $(#[$section_meta])*
            #[derive(Debug, Clone, Deserialize)]
            #[serde(deny_unknown_fields)]
            pub struct $section {
                $(
                    $(#[$field_meta])*
                    pub $field: $ty,
                )+
            }
        )+
    };
}

config_sections! {
    GameConfig {
        max_players: u32,
        max_level: u32,
    },

    WorldConfig {
        map_bound: f32,
        max_shapes: usize,
        bullet_substeps: u32,
        collision_query_rad: f32,

        tank_reload_ticks: u64,
        max_level: u32,

        view_enter_rad: f32,
        view_exit_rad: f32,

        num_teams: u8,

        chat_window_ms: u64,
        chat_burst: usize,
        chat_min_gap_ms: u64,
        chat_max_chars: usize,
    },

    PlayerConfig {
        speed: f32,
        radius: f32,
        default_health: u32,

        collision_slop: f32,
        collision_correction: f32,
        acceleration: f32,

        body_dps: f32,
        square_contact_dps: f32,
        triangle_contact_dps: f32,
        pentagon_contact_dps: f32,

        stat_max_level: u8,
        stat_max_health_bonus: u32,

        stat_regen_base: f32,
        stat_regen_per: f32,
        stat_body_dmg_per: f32,
        stat_bullet_speed_per: f32,
        stat_bullet_dmg_per: f32,
        stat_bullet_hp_per: f32,
        stat_reload_per: f32,
        stat_move_per: f32,
    },

    BulletConfig {
        size: f32,

        speed_mult: f32,
        base_dmg: f32,
        dmg_per_level: f32,
        lifetime: f32,
        base_health: f32,

        recoil_impulse: f32,
        recoil_decay: f32,
        recoil_max: f32,
    },

    AntiCheatConfig {
        min_upgrade_interval_ms: u64,
        min_aim_samples: usize,

        max_human_angular_velocity: f32,
        max_human_angular_jerk: f32,
        max_sign_inversion_ratio: f32,

        multibox_min_samples: usize,
        multibox_threshold_rad: f32,
        multibox_similarity_flag: f32,

        suspicion_decay: u32,
    },

    BotConfig {
        id_base: u32,
        count: usize,
        respawn_ticks: u64,
        bullet_strength: f32,
        names: Vec<String>,

        vision: f32,
        engage_vision: f32,
        threat_vision: f32,
        low_health_frac: f32,
        engage_power_ratio: f32,
        aggro_timeout_ticks: u64,
        wander_interval: u64,
        macro_interval: u64,
        decide_interval: u64,
        repath_interval: u64,
        kite_range_frac: f32,
        steer_smooth: f32,
        vs_bot_bias: f32,

        max_tracked_enemies: usize,
        outnumber_ratio: f32,
        support_dps_frac: f32,
        flank_radius: f32,

        skill_min: f32,
        skill_span: f32,
        noise_aim_wobble: f32,
        noise_aim_scatter: f32,
        noise_steer_jitter: f32,
        noise_power_estimate: f32,
        noise_hesitate_chance: u32,
        reaction_min: u64,
        reaction_span: u64,

        duel_max_dist: f32,
        duel_ram_dist: f32,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub game: GameConfig,
    pub world: WorldConfig,
    pub player: PlayerConfig,
    pub bullet: BulletConfig,
    pub anticheat: AntiCheatConfig,
    pub bots: BotConfig,
}

#[derive(Debug, Snafu)]
pub enum ConfigError {
    #[snafu(display("could not read config file {}: {source}", path.display()))]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },

    #[snafu(display("invalid config in {}: {source}", path.display()))]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
}

impl Config {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ConfigError> {
        let path = path.as_ref();

        let contents = fs::read_to_string(path).context(ReadSnafu {
            path: path.to_path_buf(),
        })?;

        toml::from_str(&contents).context(ParseSnafu {
            path: path.to_path_buf(),
        })
    }
}
