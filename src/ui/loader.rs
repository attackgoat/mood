use {
    super::Operation,
    crate::{
        art::open_pak,
        render::{
            bitmap::{Bitmap, BitmapBuffer},
            model::{Material, Model, ModelBuffer, ModelBufferInfo, ModelBufferTechnique},
        },
    },
    anyhow::Context,
    bmfont::{BMFont, OrdinateOrientation},
    crossbeam_channel::unbounded,
    kira::sound::static_sound::{StaticSoundData, StaticSoundSettings},
    pak::{bitmap::BitmapFormat, scene::SceneBuf, BitmapId, MaterialId, ModelId, Pak, PakBuf},
    parking_lot::Mutex,
    screen_13::prelude::*,
    screen_13_fx::{BitmapFont, ImageFormat, ImageLoader},
    std::{
        collections::{HashMap, HashSet},
        io::Cursor,
        sync::{
            atomic::{AtomicBool, AtomicUsize, Ordering},
            Arc,
        },
        thread::{spawn, JoinHandle},
    },
};

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum IdOrKey<T> {
    Id(T),
    Key(&'static str),
}

#[derive(Clone, Copy, Debug, Default)]
pub struct LoadInfo<'a> {
    pub bitmaps: &'a [&'static str],
    pub fonts: &'a [&'static str],
    pub materials: &'a [&'static str],
    pub models: &'a [&'static str],
    pub scenes: &'a [&'static str],
    pub sounds: &'a [&'static str],
}

impl<'a> LoadInfo<'a> {
    pub fn bitmaps(mut self, bitmaps: &'a [&'static str]) -> Self {
        self.bitmaps = bitmaps;
        self
    }

    pub fn fonts(mut self, fonts: &'a [&'static str]) -> Self {
        self.fonts = fonts;
        self
    }

    pub fn materials(mut self, materials: &'a [&'static str]) -> Self {
        self.materials = materials;
        self
    }

    pub fn models(mut self, models: &'a [&'static str]) -> Self {
        self.models = models;
        self
    }

    pub fn scenes(mut self, scenes: &'a [&'static str]) -> Self {
        self.scenes = scenes;
        self
    }

    pub fn sounds(mut self, sounds: &'a [&'static str]) -> Self {
        self.sounds = sounds;
        self
    }
}

pub struct Loader {
    bitmap_buf: Arc<Mutex<Option<BitmapBuffer>>>,
    bitmaps: Arc<Mutex<HashMap<&'static str, Bitmap>>>,
    err: Arc<AtomicBool>,
    fonts: Arc<Mutex<HashMap<&'static str, BitmapFont>>>,
    loaded: Arc<AtomicUsize>,
    materials: Arc<Mutex<HashMap<IdOrKey<MaterialId>, Material>>>,
    model_buf: Arc<Mutex<Option<ModelBuffer>>>,
    models: Arc<Mutex<HashMap<IdOrKey<ModelId>, Model>>>,
    threads: Vec<JoinHandle<()>>,
    total: usize,
    scenes: Arc<Mutex<HashMap<&'static str, SceneBuf>>>,
    sounds: Arc<Mutex<HashMap<&'static str, StaticSoundData>>>,
}

impl Loader {
    // TODO: This has become *way* too complicated. Need to remove the multiple points where model
    // buffer is instantiated and make simpler in general!
    pub fn spawn_threads(
        device: &Arc<Device>,
        graphics: Option<ModelBufferTechnique>,
        info: LoadInfo,
    ) -> anyhow::Result<Self> {
        #[cfg(debug_assertions)]
        {
            let mut keys = HashSet::new();

            for key in info
                .bitmaps
                .iter()
                .chain(info.fonts.iter())
                .chain(info.materials.iter())
                .chain(info.models.iter())
                .chain(info.scenes.iter())
                .chain(info.sounds.iter())
                .copied()
            {
                assert!(keys.insert(key), "Duplicate key {}", key);
            }
        }

        let mut model_buf_info = ModelBufferInfo::new();

        if let Some(graphics) = graphics {
            model_buf_info = model_buf_info.technique(graphics);
        }

        let model_buf_info = model_buf_info.build();

        let bitmap_buf: Option<BitmapBuffer> = None;
        let image_loader: Option<ImageLoader> = None;
        let model_buf: Option<ModelBuffer> = None;

        type BitmapCache = HashMap<BitmapId, Arc<Mutex<Option<(Arc<Image>, bool)>>>>;
        let bitmap_cache: BitmapCache = HashMap::new();
        let bitmap_cache = Arc::new(Mutex::new(bitmap_cache));

        let bitmap_buf = Arc::new(Mutex::new(bitmap_buf));
        let image_loader = Arc::new(Mutex::new(image_loader));
        let model_buf = Arc::new(Mutex::new(model_buf));

        let err = Arc::new(AtomicBool::new(false));
        let loaded = Arc::new(AtomicUsize::new(0));
        let mut threads = vec![];

        let bitmaps = Arc::new(Mutex::new(HashMap::new()));
        let fonts = Arc::new(Mutex::new(HashMap::new()));
        let materials = Arc::new(Mutex::new(HashMap::new()));
        let models = Arc::new(Mutex::new(HashMap::new()));
        let scenes = Arc::new(Mutex::new(HashMap::new()));
        let sounds = Arc::new(Mutex::new(HashMap::new()));

        let key_count = info.bitmaps.len()
            + info.fonts.len()
            + info.materials.len()
            + info.models.len()
            + info.scenes.len()
            + info.sounds.len();
        let queue_count = device.physical_device.queue_families[1].queue_count as usize;

        //assert!(queue_count > 1, "Unsupported single-queue device");

        let thread_count = key_count.min(queue_count);
        let (tx, rx) = unbounded();

        debug!("Loading {} keys using {} threads", key_count, thread_count);

        #[derive(Clone, Copy)]
        enum Message {
            Done,
            Bitmap(&'static str),
            Font(&'static str),
            Material(&'static str),
            Model(&'static str),
            Scene(&'static str),
            Sound(&'static str),
        }

        fn load_bitmap(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            key: &'static str,
            bitmap_cache: &Arc<Mutex<BitmapCache>>,
            image_loader: &Arc<Mutex<Option<ImageLoader>>>,
            bitmap_buf: &Arc<Mutex<Option<BitmapBuffer>>>,
            bitmaps: &Arc<Mutex<HashMap<&'static str, Bitmap>>>,
            queue_index: usize,
        ) -> anyhow::Result<()> {
            let id = pak
                .bitmap_id(key)
                .ok_or(DriverError::InvalidData)
                .context("Getting bitmap ID")?;
            let (image, has_alpha) =
                read_image(device, pak, id, bitmap_cache, image_loader, queue_index)
                    .context("Reading bitmap image")?;
            let mut bitmap_buf = bitmap_buf.lock();

            if bitmap_buf.is_none() {
                *bitmap_buf = Some(BitmapBuffer::new(device).context("Creating bitmap buffer")?);
            }

            let bitmap = bitmap_buf
                .as_mut()
                .unwrap()
                .load_bitmap(queue_index, image, has_alpha)
                .context("Loading bitmap")?;

            bitmaps.lock().insert(key, bitmap);

            Ok(())
        }

        fn load_font(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            key: &'static str,
            image_loader: &Arc<Mutex<Option<ImageLoader>>>,
            fonts: &Arc<Mutex<HashMap<&'static str, BitmapFont>>>,
            queue_index: usize,
        ) -> anyhow::Result<()> {
            let font = pak.read_bitmap_font(key).context("Reading font")?;

            let page_bufs = font.pages();
            let mut pages = Vec::with_capacity(page_bufs.len());
            for page in page_bufs {
                let mut image_loader = image_loader.lock();

                if image_loader.is_none() {
                    *image_loader =
                        Some(ImageLoader::new(device).context("Creating image loader")?);
                }

                let page = image_loader
                    .as_mut()
                    .unwrap()
                    .decode_linear(
                        0,
                        queue_index,
                        page.pixels(),
                        match page.format() {
                            BitmapFormat::Rgb => ImageFormat::R8G8B8,
                            BitmapFormat::Rgba => ImageFormat::R8G8B8A8,
                            _ => unimplemented!(),
                        },
                        page.width(),
                        page.height(),
                    )
                    .context("Loading font page image")?;
                pages.push(page);
            }

            let font = BMFont::new(Cursor::new(font.def()), OrdinateOrientation::TopToBottom)
                .context("Parsing font")?;
            let font = BitmapFont::new(device, font, pages).context("Creating font")?;

            fonts.lock().insert(key, font);

            Ok(())
        }

        fn load_material(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            key: &'static str,
            bitmap_cache: &Arc<Mutex<BitmapCache>>,
            image_loader: &Arc<Mutex<Option<ImageLoader>>>,
            model_buf: &Arc<Mutex<Option<ModelBuffer>>>,
            model_buf_info: ModelBufferInfo,
            materials: &Arc<Mutex<HashMap<IdOrKey<MaterialId>, Material>>>,
            queue_index: usize,
        ) -> anyhow::Result<()> {
            let id = pak
                .material_id(key)
                .ok_or(DriverError::InvalidData)
                .context("Getting material ID")?;
            let (color, normal, params, emissive) =
                read_material(device, pak, id, bitmap_cache, image_loader, queue_index)
                    .context("Reading material")?;

            let mut materials = materials.lock();
            let key = IdOrKey::Key(key);
            let id = IdOrKey::Id(id);

            if !materials.contains_key(&id) {
                let mut model_buf = model_buf.lock();

                if model_buf.is_none() {
                    *model_buf = Some(
                        ModelBuffer::new(device, model_buf_info)
                            .context("Creating model buffer")?,
                    );
                }

                let material = model_buf
                    .as_mut()
                    .unwrap()
                    .load_material(queue_index, color, normal, params, emissive)
                    .context("Loading material")?;

                materials.insert(id, material);
            }

            let material = materials[&id];

            if !materials.contains_key(&key) {
                materials.insert(key, material);
            }

            Ok(())
        }

        fn load_model(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            key: &'static str,
            model_buf: &Arc<Mutex<Option<ModelBuffer>>>,
            model_buf_info: ModelBufferInfo,
            models: &Arc<Mutex<HashMap<IdOrKey<ModelId>, Model>>>,
            queue_index: usize,
        ) -> anyhow::Result<()> {
            let id = pak
                .model_id(key)
                .ok_or(DriverError::InvalidData)
                .context("Getting model ID")?;
            let model = pak.read_model(key).context("Reading model")?;

            let mut models = models.lock();
            let key = IdOrKey::Key(key);
            let id = IdOrKey::Id(id);

            if !models.contains_key(&id) {
                let mut model_buf = model_buf.lock();

                if model_buf.is_none() {
                    *model_buf = Some(
                        ModelBuffer::new(device, model_buf_info)
                            .context("Creating model buffer")?,
                    );
                }

                let model = model_buf
                    .as_mut()
                    .unwrap()
                    .load_model(queue_index, model)
                    .context("Loading model")?;

                models.insert(id, model);
            }

            let model = models[&id];

            if !models.contains_key(&key) {
                models.insert(key, model);
            }

            Ok(())
        }

        fn load_scene(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            key: &'static str,
            scenes: &Arc<Mutex<HashMap<&'static str, SceneBuf>>>,
            bitmap_cache: &Arc<Mutex<BitmapCache>>,
            image_loader: &Arc<Mutex<Option<ImageLoader>>>,
            model_buf: &Arc<Mutex<Option<ModelBuffer>>>,
            model_buf_info: ModelBufferInfo,
            materials: &Arc<Mutex<HashMap<IdOrKey<MaterialId>, Material>>>,
            models: &Arc<Mutex<HashMap<IdOrKey<ModelId>, Model>>>,
            queue_index: usize,
        ) -> anyhow::Result<()> {
            let scene = pak.read_scene(key).context("Reading scene")?;

            for scene_ref in scene.refs() {
                for material_id in scene_ref.materials().iter().copied() {
                    let (color, normal, params, emissive) = read_material(
                        device,
                        pak,
                        material_id,
                        bitmap_cache,
                        image_loader,
                        queue_index,
                    )
                    .with_context(|| format!("Reading material {material_id:?}"))?;

                    let mut materials = materials.lock();
                    let material_id = IdOrKey::Id(material_id);

                    if !materials.contains_key(&material_id) {
                        let mut model_buf = model_buf.lock();

                        if model_buf.is_none() {
                            *model_buf = Some(
                                ModelBuffer::new(device, model_buf_info)
                                    .context("Creating model buffer")?,
                            );
                        }

                        let material = model_buf
                            .as_mut()
                            .unwrap()
                            .load_material(queue_index, color, normal, params, emissive)
                            .context("Loading material")?;

                        materials.insert(material_id, material);
                    }
                }

                if let Some(model_id) = scene_ref.model() {
                    let model = pak
                        .read_model_id(model_id)
                        .with_context(|| format!("Reading model {model_id:?}"))?;

                    let mut models = models.lock();
                    let model_id = IdOrKey::Id(model_id);

                    if !models.contains_key(&model_id) {
                        let mut model_buf = model_buf.lock();

                        if model_buf.is_none() {
                            *model_buf = Some(
                                ModelBuffer::new(device, model_buf_info)
                                    .context("Creating model buffer")?,
                            );
                        }

                        let model = model_buf
                            .as_mut()
                            .unwrap()
                            .load_model(queue_index, model)
                            .context("Loading model")?;

                        models.insert(model_id, model);
                    }
                }
            }

            scenes.lock().insert(key, scene);

            Ok(())
        }

        fn load_sound(
            pak: &mut PakBuf,
            key: &'static str,
            sounds: &Arc<Mutex<HashMap<&'static str, StaticSoundData>>>,
        ) -> anyhow::Result<()> {
            let sound = pak.read_blob(key).context("Reading sound")?;
            let sound =
                StaticSoundData::from_cursor(Cursor::new(sound), StaticSoundSettings::new())
                    .context("Loading sound")?;

            sounds.lock().insert(key, sound);

            Ok(())
        }

        fn read_image(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            id: BitmapId,
            bitmap_cache: &Arc<Mutex<BitmapCache>>,
            image_loader: &Arc<Mutex<Option<ImageLoader>>>,
            queue_index: usize,
        ) -> anyhow::Result<(Arc<Image>, bool)> {
            let bitmap_cache = bitmap_cache.lock().entry(id).or_default().clone();
            let mut bitmap_entry = bitmap_cache.lock();

            if bitmap_entry.is_none() {
                let bitmap = pak.read_bitmap_id(id).context("Reading bitmap")?;
                let bitmap_format = bitmap.format();
                let mut image_loader = image_loader.lock();

                if image_loader.is_none() {
                    *image_loader =
                        Some(ImageLoader::new(device).context("Creating image loader")?);
                }

                let image = image_loader
                    .as_mut()
                    .unwrap()
                    .decode_linear(
                        0,
                        queue_index,
                        bitmap.pixels(),
                        match bitmap_format {
                            BitmapFormat::R => ImageFormat::R8,
                            BitmapFormat::Rg => ImageFormat::R8G8,
                            BitmapFormat::Rgb => ImageFormat::R8G8B8,
                            BitmapFormat::Rgba => ImageFormat::R8G8B8A8,
                        },
                        bitmap.width(),
                        bitmap.height(),
                    )
                    .context("Loading image")?;

                *bitmap_entry = Some((image, bitmap_format == BitmapFormat::Rgba));
            }

            Ok(bitmap_entry
                .as_ref()
                .map(|(image, has_alpha)| (Arc::clone(image), *has_alpha))
                .unwrap())
        }

        fn read_material(
            device: &Arc<Device>,
            pak: &mut PakBuf,
            id: MaterialId,
            bitmap_cache: &Arc<Mutex<BitmapCache>>,
            image_loader: &Arc<Mutex<Option<ImageLoader>>>,
            queue_index: usize,
        ) -> anyhow::Result<(Arc<Image>, Arc<Image>, Arc<Image>, Option<Arc<Image>>)> {
            let info = pak.read_material_id(id).context("Reading material info")?;

            // Get the unique list of bitmaps in this material (In practice they are always unique!)
            let mut bitmap_ids = HashSet::with_capacity(3 + info.emissive.is_some() as usize);
            bitmap_ids.insert(info.color);
            bitmap_ids.insert(info.normal);
            bitmap_ids.insert(info.params);

            if let Some(emissive) = info.emissive {
                bitmap_ids.insert(emissive);
            }

            // Sort the bitmaps-to-read so that we don't deaclock with another thread
            let mut bitmap_ids = bitmap_ids.drain().collect::<Box<[_]>>();
            bitmap_ids.sort_unstable();

            let mut images = HashMap::with_capacity(bitmap_ids.len());
            for bitmap_id in bitmap_ids.iter().copied() {
                let (image, _) = read_image(
                    device,
                    pak,
                    bitmap_id,
                    bitmap_cache,
                    image_loader,
                    queue_index,
                )
                .context("Reading material image")?;
                images.insert(bitmap_id, image);
            }

            let color = images[&info.color].clone();
            let normal = images[&info.color].clone();
            let params = images[&info.color].clone();
            let emissive = info.emissive.map(|id| images[&id].clone());

            Ok((color, normal, params, emissive))
        }

        for thread_index in 0..thread_count {
            let err = Arc::clone(&err);
            let loaded = Arc::clone(&loaded);
            let rx = rx.clone();

            let queue_index = thread_index;

            let device = Arc::clone(device);

            let bitmap_buf = Arc::clone(&bitmap_buf);
            let bitmap_cache = Arc::clone(&bitmap_cache);
            let model_buf = Arc::clone(&model_buf);
            let image_loader = Arc::clone(&image_loader);

            let bitmaps = Arc::clone(&bitmaps);
            let fonts = Arc::clone(&fonts);
            let materials = Arc::clone(&materials);
            let models = Arc::clone(&models);
            let scenes = Arc::clone(&scenes);
            let sounds = Arc::clone(&sounds);

            threads.push(spawn(move || {
                let pak = open_pak();
                if let Err(e) = &pak {
                    error!("Pak error: {e}");

                    err.fetch_or(true, Ordering::Relaxed);
                    return;
                }

                let mut pak = pak.unwrap();

                loop {
                    if let Err(e) = match rx.recv().unwrap_or_else(|recv_err| {
                        error!("Receive error: {recv_err}");

                        err.store(true, Ordering::Relaxed);

                        Message::Done
                    }) {
                        Message::Done => break,
                        Message::Bitmap(key) => load_bitmap(
                            &device,
                            &mut pak,
                            key,
                            &bitmap_cache,
                            &image_loader,
                            &bitmap_buf,
                            &bitmaps,
                            queue_index,
                        )
                        .with_context(|| format!("Bitmap {key}")),
                        Message::Font(key) => {
                            load_font(&device, &mut pak, key, &image_loader, &fonts, queue_index)
                                .with_context(|| format!("Font {key}"))
                        }
                        Message::Material(key) => load_material(
                            &device,
                            &mut pak,
                            key,
                            &bitmap_cache,
                            &image_loader,
                            &model_buf,
                            model_buf_info,
                            &materials,
                            queue_index,
                        )
                        .with_context(|| format!("Material {key}")),
                        Message::Model(key) => load_model(
                            &device,
                            &mut pak,
                            key,
                            &model_buf,
                            model_buf_info,
                            &models,
                            queue_index,
                        )
                        .with_context(|| format!("Model {key}")),
                        Message::Scene(key) => load_scene(
                            &device,
                            &mut pak,
                            key,
                            &scenes,
                            &bitmap_cache,
                            &image_loader,
                            &model_buf,
                            model_buf_info,
                            &materials,
                            &models,
                            queue_index,
                        )
                        .with_context(|| format!("Scene {key}")),
                        Message::Sound(key) => load_sound(&mut pak, key, &sounds)
                            .with_context(|| format!("Sound {key}")),
                    } {
                        error!("Load error: {e:?}");

                        err.store(true, Ordering::SeqCst);
                        break;
                    }

                    loaded.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }

        let mut total = 0;

        for key in info.bitmaps {
            tx.send(Message::Bitmap(*key))?;
            total += 1;
        }

        for key in info.fonts {
            tx.send(Message::Font(*key))?;
            total += 1;
        }

        for key in info.models {
            tx.send(Message::Model(*key))?;
            total += 1;
        }

        for key in info.scenes {
            tx.send(Message::Scene(*key))?;
            total += 1;
        }

        for key in info.sounds {
            tx.send(Message::Sound(*key))?;
            total += 1;
        }

        for key in info.materials {
            tx.send(Message::Material(*key))?;
            total += 1;
        }

        for _ in 0..thread_count {
            tx.send(Message::Done)?;
        }

        Ok(Self {
            bitmaps,
            bitmap_buf,
            err,
            fonts,
            loaded,
            materials,
            models,
            model_buf,
            threads,
            total,
            scenes,
            sounds,
        })
    }
}

impl Operation<LoadResult> for Loader {
    fn progress(&self) -> f32 {
        let loaded = self.loaded.load(Ordering::Relaxed).min(self.total);

        loaded as f32 / self.total.max(1) as f32
    }

    fn is_done(&self) -> bool {
        let loaded = self.loaded.load(Ordering::Relaxed);
        loaded == self.total
    }

    fn is_err(&self) -> bool {
        self.err.load(Ordering::Relaxed)
    }

    fn unwrap(self: Box<Self>) -> LoadResult {
        debug_assert!(!self.is_err());
        debug_assert!(self.is_done());

        for thread in self.threads {
            thread.join().unwrap_or_default();
        }

        let bitmap_buf = Arc::try_unwrap(self.bitmap_buf).unwrap().into_inner();
        let model_buf = Arc::try_unwrap(self.model_buf).unwrap().into_inner();

        let bitmaps = Arc::try_unwrap(self.bitmaps).unwrap().into_inner();
        let fonts = Arc::try_unwrap(self.fonts).unwrap().into_inner();
        let materials = Arc::try_unwrap(self.materials).unwrap().into_inner();
        let models = Arc::try_unwrap(self.models).unwrap().into_inner();
        let scenes = Arc::try_unwrap(self.scenes).unwrap().into_inner();
        let sounds = Arc::try_unwrap(self.sounds).unwrap().into_inner();

        debug!(
            "Loaded {} keys",
            bitmaps.len()
                + fonts.len()
                + materials.len()
                + models.len()
                + scenes.len()
                + sounds.len()
        );

        LoadResult {
            bitmap_buf,
            model_buf,

            bitmaps,
            fonts,
            materials,
            models,
            scenes,
            sounds,
        }
    }
}

pub struct LoadResult {
    pub bitmap_buf: Option<BitmapBuffer>,
    pub model_buf: Option<ModelBuffer>,

    pub bitmaps: HashMap<&'static str, Bitmap>,
    pub fonts: HashMap<&'static str, BitmapFont>,
    pub materials: HashMap<IdOrKey<MaterialId>, Material>,
    pub models: HashMap<IdOrKey<ModelId>, Model>,
    pub scenes: HashMap<&'static str, SceneBuf>,
    pub sounds: HashMap<&'static str, StaticSoundData>,
}
