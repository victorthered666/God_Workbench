use std::{env, path::{Path, PathBuf}};

#[derive(Clone, Debug)]
pub struct Config {
    pub root: PathBuf,
    pub data_dir: PathBuf,
    pub output_dir: PathBuf,
    pub port: u16,
}

impl Config {
    pub fn from_env() -> crate::Result<Self> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let data_dir = env::var_os("GDD_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("data"));
        let output_dir = env::var_os("GDD_OUTPUT_DIR").map(PathBuf::from).unwrap_or_else(|| root.join("outputs"));
        let port = env::var("GDD_PORT").ok().map(|v| v.parse()).transpose().map_err(|_| crate::AppError::Invalid("GDD_PORT 必须是有效端口".into()))?.unwrap_or(8765);
        Ok(Self { root, data_dir, output_dir, port })
    }

    pub fn with_dirs(root: impl AsRef<Path>, data: impl AsRef<Path>, outputs: impl AsRef<Path>, port: u16) -> Self {
        Self { root: root.as_ref().into(), data_dir: data.as_ref().into(), output_dir: outputs.as_ref().into(), port }
    }

    pub fn init(&self) -> crate::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.output_dir)?;
        Ok(())
    }

    pub fn database(&self) -> PathBuf { self.data_dir.join("gdd.db") }
}

pub const MAX_MESSAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_TOOL_RESULT_BYTES: usize = 256 * 1024;
pub const MAX_SESSION_BYTES: usize = 6 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_IMAGE_DOWNLOAD: usize = 12 * 1024 * 1024;
pub const MAX_VISION_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_REVIEW_ROUNDS: u32 = 3;
