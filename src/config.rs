use {
    crate::{fs::project_dirs, render::model::ModelBufferTechnique},
    screen_13::prelude::*,
    serde::{de::DeserializeOwned, Deserialize, Serialize},
    std::{
        fmt::Debug,
        fs::{metadata, read_to_string, write},
        io::{Error, ErrorKind},
        path::{Path, PathBuf},
    },
};

fn default_framerate_limit() -> usize {
    60
}

fn default_graphics() -> Option<ModelBufferTechnique> {
    None
}

fn default_mouse_sensitivity() -> f32 {
    100.0
}

fn default_v_sync() -> bool {
    false
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default = "default_framerate_limit")]
    pub framerate_limit: usize,

    #[serde(default = "default_graphics")]
    pub graphics: Option<ModelBufferTechnique>,

    #[serde(default = "default_mouse_sensitivity")]
    pub mouse_sensitivity: f32,

    #[serde(default = "default_v_sync")]
    pub v_sync: bool,
}

impl Config {
    const FILE_NAME: &str = "config.toml";

    fn local_path() -> PathBuf {
        project_dirs()
            .map(|dirs| dirs.data_local_dir().to_path_buf())
            .unwrap_or_default()
            .join(Self::FILE_NAME)
    }

    pub fn read() -> Self {
        let mut res: Self = Self::read_path(Self::local_path());

        res.framerate_limit = res.framerate_limit.clamp(60, 480);

        res
    }

    fn read_path<P, T>(path: P) -> T
    where
        P: AsRef<Path>,
        T: Debug + Default + DeserializeOwned,
    {
        let config = if metadata(path.as_ref()).is_err() {
            info!("Using default config file");

            Default::default()
        } else {
            info!("Reading {}", path.as_ref().display());

            let txt = read_to_string(path).unwrap_or_else(|_| {
                warn!("Unable to read file");

                Default::default()
            });

            toml::from_str(txt.as_str()).unwrap_or_else(|_| {
                warn!("Unable to parse file");

                Default::default()
            })
        };

        info!("{:#?}", config);

        config
    }

    pub fn write(&self) -> Result<(), Error> {
        Self::write_path(Self::local_path(), self)?;

        Ok(())
    }

    fn write_path<P, T>(path: P, t: &T) -> Result<(), Error>
    where
        P: AsRef<Path>,
        T: Serialize,
    {
        trace!("Writing {}", path.as_ref().display());

        write(
            path,
            &toml::to_string(t).map_err(|_| Error::from(ErrorKind::InvalidData))?,
        )?;

        Ok(())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            framerate_limit: default_framerate_limit(),
            graphics: default_graphics(),
            mouse_sensitivity: default_mouse_sensitivity(),
            v_sync: default_v_sync(),
        }
    }
}
