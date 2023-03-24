use {
    self::tools::*,
    anyhow::{bail, Context},
    lazy_static::lazy_static,
    log::{error, info, trace},
    pak::PakBuf,
    shaderc::{CompileOptions, EnvVersion, SpirvVersion, TargetEnv},
    simplelog::{CombinedLogger, ConfigBuilder, LevelFilter, WriteLogger},
    std::{
        collections::HashMap,
        env::var,
        fs::{metadata, read_dir, remove_file, write, File, OpenOptions},
        path::{Path, PathBuf, MAIN_SEPARATOR},
        process::Command,
        time::SystemTime,
    },
};

type Timestamps = HashMap<PathBuf, SystemTime>;

lazy_static! {
    static ref CARGO_MANIFEST_DIR: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    static ref OUT_DIR: PathBuf = PathBuf::from(var("OUT_DIR").unwrap());
    static ref TARGET_DIR: PathBuf = OUT_DIR
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    static ref TIMESTAMPS_PATH: PathBuf = CARGO_MANIFEST_DIR.join(".timestamps");
}

#[cfg(target_os = "linux")]
lazy_static! {
    static ref BLENDER_PATH: PathBuf = PathBuf::from("/snap/bin/blender");
}

#[cfg(target_os = "macos")]
lazy_static! {
    static ref BLENDER_PATH: PathBuf =
        PathBuf::from("/Applications/Blender.app/Contents/MacOS/Blender");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
lazy_static! {
    static ref FONTBM_PATH: PathBuf = CARGO_MANIFEST_DIR.join("bin/fontbm.sh");
}

#[cfg(target_os = "windows")]
lazy_static! {
    static ref BLENDER_PATH: PathBuf =
        PathBuf::from("c:\\Program Files\\Blender Foundation\\Blender 3.4\\blender.exe");
    static ref FONTBM_PATH: PathBuf = CARGO_MANIFEST_DIR.join("bin/fontbm.bat");
}

#[allow(unused)]
mod tools {
    use core::time;

    use super::*;

    pub fn glob(
        patterns: impl IntoIterator<Item = impl AsRef<str>>,
    ) -> anyhow::Result<Vec<PathBuf>> {
        let mut paths = vec![];
        for pattern in patterns {
            paths.extend(glob::glob(pattern.as_ref())?.collect::<Result<Vec<_>, _>>()?);
        }

        Ok(paths)
    }

    pub fn has_changed(path: impl AsRef<Path>, timestamps: &Timestamps) -> bool {
        rerun_if_changed(&path);

        let metadata = metadata(&path);
        if metadata.is_err() {
            trace!("Metadata not found for {}", path.as_ref().display());

            return true;
        }

        let metadata = metadata.unwrap();
        let modified = metadata.modified();
        if modified.is_err() {
            trace!("Modified time not found for {}", path.as_ref().display());

            return true;
        }

        let modified = modified.unwrap();
        let timestamp = timestamps.get(path.as_ref());
        if timestamp.is_none() {
            trace!("Timestamp not found for {}", path.as_ref().display());

            return true;
        }

        let timestamp = *timestamp.unwrap();
        let res = modified != timestamp;

        trace!(
            "Timestamp changed = {} for {} ({:?})",
            res,
            path.as_ref().display(),
            timestamp,
        );

        res
    }

    pub fn rerun_if_changed(path: impl AsRef<Path>) {
        println!(
            "cargo:rerun-if-changed={}",
            CARGO_MANIFEST_DIR.join(path).display()
        );
    }

    // Given two paths, returns the strings of the unique parts of the given path only:
    // "c:\foo\bar" and "c:\foo\bar\baz\bop.txt" will return "baz\bop.txt"
    pub fn remove_common_path(
        common_path: impl AsRef<Path>,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<PathBuf> {
        let common_path = common_path.as_ref().to_path_buf();
        let mut path = path.as_ref().to_path_buf();
        let mut res = vec![];
        while path != common_path {
            res.push(
                path.file_name()
                    .context("Getting filename")?
                    .to_string_lossy()
                    .to_string(),
            );
            path = path.parent().context("Getting parent")?.to_path_buf();
        }

        Ok(PathBuf::from(
            res.into_iter()
                .rev()
                .collect::<Vec<_>>()
                .join(&MAIN_SEPARATOR.to_string()),
        ))
    }

    pub fn report_pak(path: impl AsRef<Path>) -> anyhow::Result<()> {
        info!(".pak Report: {}", path.as_ref().display());
        info!(
            "File size: {} MB",
            metadata(path.as_ref())
                .context("Reading .pak metadata")?
                .len()
                / 1000
                / 1000
        );

        let pak = PakBuf::open(path.as_ref()).context("Reading pak")?;
        let mut keys: Vec<&str> = Vec::new();
        for key in pak.keys() {
            if let Err(idx) = keys.binary_search_by(|probe| probe.cmp(&key)) {
                keys.insert(idx, key);
            } else {
                bail!("Non-unique keys!");
            }
        }

        for key in keys {
            info!("{key}");
        }

        Ok(())
    }

    pub fn write_pak_bindings(src: impl AsRef<Path>, dst: impl AsRef<Path>) -> anyhow::Result<()> {
        let mut bindings = String::new();
        for key in PakBuf::open(src)?.keys() {
            bindings.push_str("pub const ");
            bindings.push_str(
                key.to_ascii_uppercase()
                    .replace(['\\', '/', '-', '.', '!'], "_")
                    .as_str(),
            );
            bindings.push_str(": &str = r#\"");
            bindings.push_str(key);
            bindings.push_str("\"#;\n");
        }

        write(&dst, bindings)?;

        info!("Wrote bindings to {}", dst.as_ref().display());

        Ok(())
    }
}

fn main() -> anyhow::Result<()> {
    let mut log_config = ConfigBuilder::new();
    log_config.set_time_offset_to_local().unwrap();
    CombinedLogger::init(vec![WriteLogger::new(
        LevelFilter::Trace,
        log_config.build(),
        File::create("build.log").unwrap(),
    )])
    .unwrap();

    info!("Build started");

    if let Err(err) = build() {
        error!("Build failed: {}", err);

        Err(err)
    } else {
        info!("Build complete");

        Ok(())
    }
}

fn build() -> anyhow::Result<()> {
    if metadata(CARGO_MANIFEST_DIR.join("art/scene/level_01.blend"))?.len() < 1024 {
        bail!("Git LFS objects have not been downloaded; see README.md");
    }

    let mut timestamps: Timestamps = bincode::deserialize_from(
        OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .open(TIMESTAMPS_PATH.as_path())
            .context("Reading timestamps")?,
    )
    .unwrap_or_default();

    let changed = build_fonts(&mut timestamps).context("Building fonts")?
        | export_models(&mut timestamps).context("Exporting models")?
        | export_scenes(&mut timestamps).context("Exporting scenes")?;
    bake_pak("art", &mut timestamps, changed)?;

    let changed = compile_shaders(&mut timestamps)?;
    bake_pak("res", &mut timestamps, changed)?;

    for (path, timestamp) in &timestamps {
        trace!("Watching {} ({:?})", path.display(), timestamp);
    }

    write(
        TIMESTAMPS_PATH.as_path(),
        bincode::serialize(&timestamps).context("Serializing")?,
    )
    .context("Writing timestamps")?;

    Ok(())
}

fn bake_pak(
    name: impl AsRef<Path>,
    timestamps: &mut Timestamps,
    force_build: bool,
) -> anyhow::Result<()> {
    let toml = CARGO_MANIFEST_DIR.join(&name).join("pak.toml");

    rerun_if_changed(&toml);

    let pak = TARGET_DIR.join(name.as_ref().with_extension("pak"));

    if force_build || metadata(&pak).is_err() || has_changed(&toml, timestamps) {
        info!("Baking pak {} (forced = {})", toml.display(), force_build);

        PakBuf::bake(&toml, &pak).context("Baking pak")?;
        timestamps.insert(toml.clone(), metadata(&toml)?.modified()?);

        info!("Wrote pak");
    }

    let bindings = OUT_DIR.join(name.as_ref().with_extension("rs"));
    write_pak_bindings(&pak, bindings).context("Writing pak bindings")?;

    rerun_if_changed(&pak);
    report_pak(&pak)?;

    Ok(())
}

fn build_fonts(timestamps: &mut Timestamps) -> anyhow::Result<bool> {
    rerun_if_changed(FONTBM_PATH.as_path());

    let fonts = glob([
        CARGO_MANIFEST_DIR.join("art/font/*.ttf").to_str().unwrap(),
        CARGO_MANIFEST_DIR.join("art/font/*.toml").to_str().unwrap(),
    ])
    .context("Reading fonts")?;

    let mut has_changes = false;
    for entry in &fonts {
        rerun_if_changed(entry);
        has_changes |= has_changed(entry, timestamps);

        if entry
            .extension()
            .unwrap_or_default()
            .to_str()
            .unwrap_or_default()
            == "ttf"
        {
            let fnt_path = entry.clone().with_extension("fnt");
            if metadata(fnt_path).is_err() {
                has_changes = true;
            }
        }
    }

    if !has_changes {
        return Ok(false);
    }

    info!("Building bitmap fonts");

    let mut generate_fonts = Command::new(FONTBM_PATH.as_os_str());

    #[cfg(target_os = "linux")]
    let generate_fonts = generate_fonts.arg("linux");

    #[cfg(target_os = "macos")]
    let generate_fonts = generate_fonts.arg("macos");

    let mut generate_fonts = generate_fonts
        .current_dir(CARGO_MANIFEST_DIR.as_path())
        .spawn()
        .context("Spawning fontbm")?;
    if !generate_fonts.wait().context("Running fontbm")?.success() {
        bail!("fontbm failed");
    }

    for entry in &fonts {
        timestamps.insert(entry.clone(), metadata(entry)?.modified()?);
    }

    Ok(has_changes)
}

#[allow(unused)]
fn build_fonts_experimental(timestamps: &mut Timestamps) -> anyhow::Result<bool> {
    use {
        raster_fonts::{font_to_image, Args as RasterFontArgs},
        serde::Deserialize,
        std::fs::read_to_string,
    };

    #[derive(Deserialize)]
    pub struct FontInfo {
        #[serde(rename = "font")]
        fonts: Vec<Font>,
    }

    #[derive(Deserialize)]
    pub struct Font {
        charset: Option<String>,
        output: String,
        output_size: Option<u32>,
        padding: Option<u32>,
        src: String,
        size: f32,
    }

    // Watch for changes to the build.toml which drives this code
    let build_toml_path = CARGO_MANIFEST_DIR.join("art/font/build.toml");
    let mut has_changes = has_changed(&build_toml_path, timestamps);
    timestamps.insert(
        build_toml_path.clone(),
        metadata(&build_toml_path)?.modified()?,
    );

    for font in toml::from_str::<FontInfo>(&read_to_string(build_toml_path)?)?.fonts {
        // Watch for changes to the font source file
        let font_src_path = CARGO_MANIFEST_DIR
            .join("art/font")
            .join(&font.src)
            .canonicalize()?;
        has_changes |= has_changed(&font_src_path, timestamps);
        timestamps.insert(font_src_path.clone(), metadata(&font_src_path)?.modified()?);

        let font_output_path = CARGO_MANIFEST_DIR.join("art/font").join(&font.output);

        // Watch for changes to the font pak-toml file
        let mut font_toml_path = font_output_path.clone();
        font_toml_path.set_extension("toml");
        has_changes |= has_changed(&font_toml_path, timestamps);
        timestamps.insert(
            font_toml_path.clone(),
            metadata(&font_toml_path)?.modified()?,
        );

        let mut font_img_path = font_output_path.clone();
        font_img_path.set_extension("png");

        if has_changes || metadata(&font_img_path).is_err() {
            let charset = font
                .charset
                .map(|s| {
                    s.chars()
                        .map(|c| format!("{:X}", c as u32))
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| vec![format!("{:X}-{:X}", 32, 127)]);

            font_to_image(RasterFontArgs {
                font_path: font_src_path.to_str().unwrap().to_owned(),
                img_path: font_img_path.to_str().unwrap().to_owned(),
                meta_path: font_output_path.to_str().unwrap().to_owned(),
                charset,
                coverage_levels: Some(1),
                scale: font.size,
                padding: font.padding.unwrap_or(8),
                output_image_size: font.output_size.unwrap_or(512),
                skip_kerning_table: false,
            });
        }
    }

    Ok(has_changes)
}

fn compile_shaders(timestamps: &mut Timestamps) -> anyhow::Result<bool> {
    use {serde::Deserialize, std::fs::read_to_string};

    #[derive(Deserialize)]
    pub struct ShaderInfo {
        shader: Shader,
    }

    #[derive(Deserialize)]
    pub struct Shader {
        #[serde(rename = "version")]
        versions: Vec<Version>,
    }

    #[derive(Deserialize)]
    pub struct Version {
        name: String,
        macros: Vec<String>,
    }

    fn compile_shader(
        path: impl AsRef<Path>,
        macro_definitions: &[(&str, Option<&str>)],
    ) -> anyhow::Result<Vec<u8>> {
        info!("Compiling: {}", path.as_ref().display());

        use shaderc::{Compiler, ShaderKind};

        fn read_shader_source(path: impl AsRef<Path>) -> String {
            use shader_prepper::{
                process_file, BoxedIncludeProviderError, IncludeProvider, ResolvedInclude,
                ResolvedIncludePath,
            };

            struct FileIncludeProvider;

            impl IncludeProvider for FileIncludeProvider {
                type IncludeContext = PathBuf;

                fn get_include(
                    &mut self,
                    path: &ResolvedIncludePath,
                ) -> Result<String, BoxedIncludeProviderError> {
                    rerun_if_changed(&path.0);

                    Ok(read_to_string(&path.0)?)
                }

                fn resolve_path(
                    &self,
                    path: &str,
                    context: &Self::IncludeContext,
                ) -> Result<ResolvedInclude<Self::IncludeContext>, BoxedIncludeProviderError>
                {
                    let path = context.join(path);

                    Ok(ResolvedInclude {
                        resolved_path: ResolvedIncludePath(
                            path.to_str().unwrap_or_default().to_string(),
                        ),
                        context: path
                            .parent()
                            .map(|path| path.to_path_buf())
                            .unwrap_or_else(PathBuf::new),
                    })
                }
            }

            process_file(
                path.as_ref().to_string_lossy().as_ref(),
                &mut FileIncludeProvider,
                PathBuf::new(),
            )
            .unwrap()
            .iter()
            .map(|chunk| chunk.source.as_str())
            .collect()
        }

        let path = path.as_ref().to_path_buf();
        let source_code = read_shader_source(&path);

        let mut addl_opts =
            CompileOptions::new().expect("Unable to create additional compiler options");
        addl_opts.set_target_env(TargetEnv::Vulkan, EnvVersion::Vulkan1_2 as u32);
        addl_opts.set_target_spirv(SpirvVersion::V1_5);

        for (name, value) in macro_definitions.iter().copied() {
            addl_opts.add_macro_definition(name, value);
        }

        #[cfg(target_os = "macos")]
        addl_opts.add_macro_definition("MOLTEN_VK", Some("1"));

        let compiler = Compiler::new().unwrap();
        let spirv_code = compiler
            .compile_into_spirv(
                &source_code,
                match path
                    .extension()
                    .map(|ext| ext.to_string_lossy().to_string())
                    .unwrap_or_default()
                    .as_str()
                {
                    "comp" => ShaderKind::Compute,
                    "frag" => ShaderKind::Fragment,
                    "vert" => ShaderKind::Vertex,
                    "rgen" => ShaderKind::RayGeneration,
                    "rchit" => ShaderKind::ClosestHit,
                    "rmiss" => ShaderKind::Miss,
                    _ => unimplemented!(),
                },
                &path.to_string_lossy(),
                "main",
                Some(&addl_opts),
            )
            .map_err(|err| {
                eprintln!("Shader: {}", path.display());

                for (idx, line) in source_code.split('\n').enumerate() {
                    eprintln!("{}: {line}", idx + 1);
                }

                eprintln!();

                err
            })?
            .as_binary_u8()
            .to_vec();

        Ok(spirv_code)
    }

    let shader_dir = CARGO_MANIFEST_DIR.join("res/shader/**");

    let glsl_paths = glob([shader_dir.join("*.glsl").to_string_lossy()])?;
    let shader_paths = glob(
        ["*.comp", "*.vert", "*.frag", "*.rgen", "*.rchit", "*.rmiss"]
            .into_iter()
            .map(|path| shader_dir.join(path).to_string_lossy().to_string()),
    )?;

    let mut has_changes = false;
    for path in glsl_paths.iter().chain(&shader_paths) {
        if has_changed(path, timestamps) {
            has_changes = true;
            break;
        }

        let toml_path = path.with_extension("toml");
        if metadata(&toml_path).is_ok() && has_changed(&toml_path, timestamps) {
            has_changes = true;
            break;
        }
    }

    if !has_changes {
        info!("No shader changes found");

        return Ok(false);
    }

    for shader_path in &shader_paths {
        let toml_path = shader_path.with_extension("toml");
        if metadata(&toml_path).is_ok() {
            let shader_info: ShaderInfo = toml::from_str(&read_to_string(&toml_path)?)
                .with_context(|| format!("Reading shader version file: {}", toml_path.display()))?;

            for shader_version in &shader_info.shader.versions {
                let macro_definitions = shader_version
                    .macros
                    .iter()
                    .map(|macro_definition| {
                        let mut parts = macro_definition.split('=');
                        let name = parts.next().unwrap();
                        let value = parts.next().unwrap();
                        (name, if value.is_empty() { None } else { Some(value) })
                    })
                    .collect::<Box<_>>();
                let spirv_path = shader_path.with_file_name(format!(
                    "{}.{}.spirv",
                    shader_path.file_name().unwrap().to_string_lossy(),
                    shader_version.name,
                ));
                write(spirv_path, compile_shader(shader_path, &macro_definitions)?)?;
            }
        } else {
            let spirv_path = shader_path.with_file_name(format!(
                "{}.spirv",
                shader_path.file_name().unwrap().to_string_lossy(),
            ));
            write(spirv_path, compile_shader(shader_path, &[])?)?;
        }
    }

    for path in glsl_paths.into_iter().chain(shader_paths) {
        timestamps.insert(path.clone(), metadata(path)?.modified()?);
    }

    Ok(true)
}

fn export_models(timestamps: &mut Timestamps) -> anyhow::Result<bool> {
    rerun_if_changed("bin/blender_export_glb.py");

    let mut has_changes = false;
    for entry in glob([CARGO_MANIFEST_DIR
        .join("art/model/**/*.blend")
        .to_str()
        .unwrap()])
    .context("Reading models")?
    {
        rerun_if_changed(&entry);

        let mut glb_path = entry.clone();
        glb_path.set_extension("glb");
        if has_changed(&entry, timestamps) {
            has_changes = true;

            if metadata(&glb_path).is_ok() {
                remove_file(&glb_path)?;
            }

            info!("Exporting {}", glb_path.display());

            let mut blender = Command::new(BLENDER_PATH.as_os_str())
                .arg(entry.as_os_str().to_string_lossy().as_ref())
                .arg("--background")
                .args(["--python-exit-code", "1"])
                .args(["--python", "bin/blender_export_glb.py"])
                .arg("--")
                .arg(glb_path.as_os_str().to_string_lossy().as_ref())
                .current_dir(CARGO_MANIFEST_DIR.as_path())
                .spawn()
                .context("Spawning blender")?;
            if !blender.wait().context("Running blender")?.success() {
                bail!("Blender failed");
            }

            timestamps.insert(entry.clone(), metadata(&entry)?.modified()?);
        }
    }

    Ok(has_changes)
}

fn export_scenes(timestamps: &mut Timestamps) -> anyhow::Result<bool> {
    rerun_if_changed("bin/blender_export_scene.py");

    let mut has_changes = false;
    for entry in read_dir(CARGO_MANIFEST_DIR.join("art/scene")).context("Reading scenes")? {
        let entry = entry.context("Reading scene")?;
        let entry_path = entry.path();

        if !matches!(entry_path.extension(), Some(ext) if ext.to_string_lossy() == "blend") {
            continue;
        }

        rerun_if_changed(&entry_path);

        let mut toml_path = entry_path.clone();
        toml_path.set_extension("toml");
        if has_changed(&entry_path, timestamps) || has_changed(&toml_path, timestamps) {
            has_changes = true;

            if metadata(&toml_path).is_ok() {
                remove_file(&toml_path)?;
            }

            info!("Exporting {}", toml_path.display());

            let mut blender = Command::new(BLENDER_PATH.as_os_str())
                .arg(entry_path.as_os_str().to_string_lossy().as_ref())
                .arg("--background")
                .args(["--python-exit-code", "1"])
                .args(["--python", "bin/blender_export_scene.py"])
                .arg("--")
                .arg(toml_path.as_os_str().to_string_lossy().as_ref())
                .current_dir(CARGO_MANIFEST_DIR.as_path())
                .spawn()
                .context("Spawning blender")?;
            if !blender.wait().context("Running blender")?.success() {
                bail!("Blender failed");
            }

            timestamps.insert(entry_path.clone(), metadata(&entry_path)?.modified()?);
        }
    }

    Ok(has_changes)
}
