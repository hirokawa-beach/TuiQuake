use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::{graphics::MapMode, map::Layer};

#[derive(Clone, Copy, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StationGrouping {
    Prefecture,
    #[default]
    Intensity,
}

impl StationGrouping {
    pub fn label(self) -> &'static str {
        match self {
            Self::Prefecture => "都道府県別",
            Self::Intensity => "震度別",
        }
    }

    pub fn other(self) -> Self {
        match self {
            Self::Prefecture => Self::Intensity,
            Self::Intensity => Self::Prefecture,
        }
    }
}

pub const DEFAULT_HISTORY_ROWS: usize = 10;
pub const MAX_HISTORY_ROWS: usize = 50;

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, Serialize)]
#[serde(default)]
pub struct Settings {
    pub map: MapMode,
    pub national: bool,
    pub layer: Layer,
    pub station_grouping: StationGrouping,
    pub history_rows: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            map: MapMode::default(),
            national: false,
            layer: Layer::default(),
            station_grouping: StationGrouping::default(),
            history_rows: DEFAULT_HISTORY_ROWS,
        }
    }
}

impl Settings {
    pub fn path() -> Option<PathBuf> {
        let directory = if cfg!(windows) {
            env::var_os("APPDATA")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        } else {
            env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .or_else(|| env::home_dir().map(|home| home.join(".config")))
        }?;
        let app_directory = if cfg!(windows) {
            "TuiQuake"
        } else {
            "tuiquake"
        };
        Some(directory.join(app_directory).join("settings.json"))
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        match fs::read(path) {
            Ok(bytes) => {
                let mut settings: Self =
                    serde_json::from_slice(&bytes).map_err(io::Error::other)?;
                settings.history_rows = settings.history_rows.clamp(1, MAX_HISTORY_ROWS);
                Ok(settings)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(error),
        }
    }

    pub fn save(self, path: &Path) -> io::Result<()> {
        let data = serde_json::to_vec_pretty(&self).map_err(io::Error::other)?;
        if let Some(directory) = path.parent() {
            fs::create_dir_all(directory)?;
        }
        let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
        // Keep the previous file intact if writing the new settings fails.
        let result = fs::write(&temporary, data).and_then(|()| fs::rename(&temporary, path));
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}
