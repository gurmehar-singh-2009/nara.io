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
        size: f32,
        max_shapes: usize,
    },

    PlayerConfig {
        speed: f32,
        radius: f32,
        default_health: u32,
    },

    BulletConfig {
        size: f32,
        lifetime: f32,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub game: GameConfig,
    pub world: WorldConfig,
    pub player: PlayerConfig,
    pub bullet: BulletConfig,
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
