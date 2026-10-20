//! Pinned whole-pipeline libqwen binding. No tensor-level bridge or Python runtime.
use crate::conditioning::{hash_file, ConditioningBackend, ConditioningScope, FrozenConditioning};
use crate::synthesis::{ggml_device, EngineOptions, Sampling, Stop};
use anyhow::{bail, Context, Result};
use libloading::Library;
use std::{
    ffi::{c_char, c_void, CStr, CString},
    path::{Path, PathBuf},
    ptr,
};

pub(crate) const REVISION: &str = "5916bf7";
const ABI: i32 = 6;
const RATE: u32 = 24000;
const HOP: usize = 1920;
type Chunk = extern "C" fn(*const f32, i32, *mut c_void) -> bool;
type Cancel = extern "C" fn(*mut c_void) -> bool;
#[repr(C)]
struct Init {
    abi: i32,
    talker: *const c_char,
    codec: *const c_char,
    fa: bool,
    clamp: bool,
    batch: i32,
    codec_chunk: f32,
    device: *const c_char,
}
#[repr(C)]
struct Params {
    abi: i32,
    text: *const c_char,
    lang: *const c_char,
    instruct: *const c_char,
    speaker: *const c_char,
    reference: *const f32,
    reference_len: i32,
    transcript: *const c_char,
    seed: i64,
    max_tokens: i32,
    temperature: f32,
    top_k: i32,
    top_p: f32,
    penalty: f32,
    sub_temperature: f32,
    sub_top_k: i32,
    sub_top_p: f32,
    dump: *const c_char,
    cancel: Option<Cancel>,
    cancel_user: *mut c_void,
    chunk: Option<Chunk>,
    chunk_user: *mut c_void,
    embedding: *const f32,
    embedding_dim: i32,
    codes: *const i32,
    reference_frames: i32,
}
#[repr(C)]
struct Voice {
    embedding: *mut f32,
    dim: i32,
    codes: *mut i32,
    frames: i32,
    codebooks: i32,
}
impl Default for Voice {
    fn default() -> Self {
        Self {
            embedding: ptr::null_mut(),
            dim: 0,
            codes: ptr::null_mut(),
            frames: 0,
            codebooks: 0,
        }
    }
}
#[repr(C)]
struct Audio {
    samples: *mut f32,
    count: i32,
    rate: i32,
    channels: i32,
}
struct Api {
    init: unsafe extern "C" fn(*const Init) -> *mut c_void,
    free: unsafe extern "C" fn(*mut c_void),
    model_type: unsafe extern "C" fn(*const c_void) -> *const c_char,
    extract: unsafe extern "C" fn(*mut c_void, *const f32, i32, *mut Voice) -> i32,
    free_voice: unsafe extern "C" fn(*mut Voice),
    synthesize: unsafe extern "C" fn(*mut c_void, *const Params, *mut Audio) -> i32,
    free_audio: unsafe extern "C" fn(*mut Audio),
    error: unsafe extern "C" fn() -> *const c_char,
    _library: Library,
}
unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
    // The pinned API signature and symbol name are checked together at load time.
    Ok(*unsafe { library.get::<T>(name) }?)
}
fn ctext(value: &str) -> Result<CString> {
    CString::new(value).context("native argument contains NUL")
}
fn path_text(path: &Path) -> Result<CString> {
    ctext(
        path.to_str()
            .context("native runtime requires a UTF-8 path")?,
    )
}
fn error_text(api: &Api) -> String {
    let p = unsafe { (api.error)() };
    if p.is_null() {
        return "native operation failed".into();
    }
    unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
}
/// Only explicit trusted paths or executable-relative installation directories.
fn library_path(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return Ok(path.to_owned());
    }
    let name = if cfg!(windows) {
        "qwen.dll"
    } else if cfg!(target_os = "macos") {
        "libqwen.dylib"
    } else {
        "libqwen.so"
    };
    let executable = std::env::current_exe()?;
    let parent = executable.parent().context("executable has no parent")?;
    for path in [
        parent.join(name),
        parent.join("lib").join(name),
        parent.join("runtime").join(name),
    ] {
        if path.is_file() {
            return Ok(path);
        }
    }
    bail!("GGML runtime missing: install the matching spqx runtime bundle or set engine.library / --engine-library (not searched in the working directory)")
}
impl Api {
    fn load(path: &Path) -> Result<Self> {
        // Loading a native library is privileged local execution, selected by user config or the installed bundle.
        let path = path
            .canonicalize()
            .with_context(|| format!("loading {}", path.display()))?;
        #[cfg(not(windows))]
        let library = unsafe { Library::new(&path) }
            .with_context(|| format!("loading {}", path.display()))?;
        #[cfg(windows)]
        let library: Library = unsafe {
            // Search this DLL's directory and safe platform defaults, never ambient CWD.
            libloading::os::windows::Library::load_with_flags(&path, 0x100 | 0x1000)
        }
        .with_context(|| format!("loading {}", path.display()))?
        .into();
        let version: unsafe extern "C" fn() -> *const c_char =
            unsafe { symbol(&library, b"qt_version\0") }?;
        let value = unsafe { version() };
        if value.is_null() {
            bail!("native runtime returned no version");
        }
        let version = unsafe { CStr::from_ptr(value) }.to_string_lossy();
        if !version.starts_with(REVISION) {
            bail!("unsupported libqwen build {version}; expected pinned source {REVISION}");
        }
        // Never call foreign *_default_params writers: a newer library could write past our ABI layout.
        unsafe {
            Ok(Self {
                init: symbol(&library, b"qt_init\0")?,
                free: symbol(&library, b"qt_free\0")?,
                model_type: symbol(&library, b"qt_model_type\0")?,
                extract: symbol(&library, b"qt_extract_voice_ref\0")?,
                free_voice: symbol(&library, b"qt_voice_ref_free\0")?,
                synthesize: symbol(&library, b"qt_synthesize\0")?,
                free_audio: symbol(&library, b"qt_audio_free\0")?,
                error: symbol(&library, b"qt_last_error\0")?,
                _library: library,
            })
        }
    }
}

pub(crate) struct Session {
    api: Api,
    context: *mut c_void,
    voice: Voice,
    conditioning: Conditioning,
    language: CString,
    model_path: PathBuf,
    codec_path: PathBuf,
}
enum Conditioning {
    Reference(CString),
    Frozen { values: Vec<f32>, seed: i64 },
    Description { instruction: CString, seed: i64 },
}
impl Conditioning {
    fn expected_model_type(&self) -> &'static str {
        match self {
            Self::Reference(_) | Self::Frozen { .. } => "base",
            Self::Description { .. } => "voice_design",
        }
    }
    // Unused fields MUST be null: even an empty nonnull reference string is invalid for VoiceDesign.
    fn fields(&self) -> (*const c_char, *const c_char, i64) {
        match self {
            Self::Reference(transcript) => (transcript.as_ptr(), ptr::null(), -1),
            Self::Frozen { seed, .. } => (ptr::null(), ptr::null(), *seed),
            Self::Description { instruction, seed } => (ptr::null(), instruction.as_ptr(), *seed),
        }
    }
    fn check_model_type(&self, actual: &str) -> Result<()> {
        let expected = self.expected_model_type();
        if actual != expected {
            bail!("operation requires {expected} checkpoint, loaded {actual}; no preset/reference substitution");
        }
        Ok(())
    }
}
impl Drop for Session {
    fn drop(&mut self) {
        // qt_free joins its compute worker; library remains loaded until after both releases.
        unsafe {
            (self.api.free_voice)(&mut self.voice);
            (self.api.free)(self.context);
        }
    }
}
fn assets(model: &Path, codec: Option<&Path>) -> Result<(PathBuf, PathBuf)> {
    let talker = if model.is_dir() {
        model.join("qwen-talker-0.6b-base-Q8_0.gguf")
    } else {
        model.to_owned()
    };
    let codec = codec.map(Path::to_owned).unwrap_or_else(|| {
        talker
            .parent()
            .unwrap_or(Path::new("."))
            .join("qwen-tokenizer-12hz-F32.gguf")
    });
    if !talker.is_file() || !codec.is_file() {
        bail!("GGML requires local talker and codec GGUF files; set model.path and engine.codec");
    }
    Ok((talker, codec))
}
impl Session {
    pub(crate) fn load(
        model: &Path,
        reference: &Path,
        transcript: &str,
        language: &str,
        options: &EngineOptions,
    ) -> Result<Self> {
        let mut session = Self::initialize(
            model,
            language,
            options,
            Conditioning::Reference(ctext(transcript)?),
        )?;
        session.prepare_reference(reference)?;
        Ok(session)
    }
    pub(crate) fn load_voice_design(
        model: &Path,
        description: &str,
        language: &str,
        seed: i64,
        options: &EngineOptions,
    ) -> Result<Self> {
        Self::initialize(
            model,
            language,
            options,
            Conditioning::Description {
                instruction: ctext(description)?,
                seed,
            },
        )
    }
    pub(crate) fn load_frozen(
        model: &Path,
        frozen: &FrozenConditioning,
        language: &str,
        seed: i64,
        options: &EngineOptions,
    ) -> Result<Self> {
        let (talker, codec) = assets(model, options.codec.as_deref())?;
        let expected = native_scope(&talker, &codec, frozen.values.len())?;
        frozen.require_scope(&expected)?;
        Self::initialize(
            model,
            language,
            options,
            Conditioning::Frozen {
                values: frozen.values.clone(),
                seed,
            },
        )
    }
    /// Copy the exact native speaker vector; does not carry ICL codes or normalize it.
    pub(crate) fn frozen_conditioning(&self) -> Result<FrozenConditioning> {
        let values = match &self.conditioning {
            Conditioning::Frozen {values, ..} => values.clone(),
            Conditioning::Reference(_) => {
                if self.voice.embedding.is_null() || !(2..=8192).contains(&self.voice.dim) {
                    bail!("native speaker encoder returned an invalid vector");
                }
                // Pinned ABI owns dim finite-readable values until this session is dropped.
                unsafe { std::slice::from_raw_parts(self.voice.embedding, self.voice.dim as usize) }.to_vec()
            },
            Conditioning::Description { .. } => bail!("VoiceDesign has no directly exportable Base speaker vector; audition then encode with the chosen Base checkpoint"),
        };
        let result = FrozenConditioning {
            version: 1,
            scope: native_scope(&self.model_path, &self.codec_path, values.len())?,
            values,
            recipe: None,
        };
        result.validate()?;
        Ok(result)
    }
    fn initialize(
        model: &Path,
        language: &str,
        options: &EngineOptions,
        conditioning: Conditioning,
    ) -> Result<Self> {
        let (talker, codec) = assets(model, options.codec.as_deref())?;
        let api = Api::load(&library_path(options.library.as_deref())?)?;
        let model_path = talker.canonicalize()?;
        let codec_path = codec.canonicalize()?;
        let talker = path_text(&model_path)?;
        let codec = path_text(&codec_path)?;
        let device = ggml_device(&options.device)?
            .map(|v| ctext(&v))
            .transpose()?;
        let init = Init {
            abi: ABI,
            talker: talker.as_ptr(),
            codec: codec.as_ptr(),
            fa: true,
            clamp: false,
            batch: 1,
            codec_chunk: 24.0,
            device: device.as_ref().map_or(ptr::null(), |v| v.as_ptr()),
        };
        let language = ctext(language)?;
        let context = unsafe { (api.init)(&init) };
        if context.is_null() {
            bail!("libqwen initialization: {}", error_text(&api));
        }
        // Own the context before validation/preparation: all failures release the model.
        let session = Self {
            api,
            context,
            voice: Voice::default(),
            conditioning,
            language,
            model_path,
            codec_path,
        };
        let actual = unsafe { (session.api.model_type)(session.context) };
        if actual.is_null() {
            bail!("libqwen returned no checkpoint type");
        }
        let actual = unsafe { CStr::from_ptr(actual) }
            .to_str()
            .context("invalid native model type")?;
        session.conditioning.check_model_type(actual)?;
        Ok(session)
    }
    fn prepare_reference(&mut self, reference: &Path) -> Result<()> {
        let (samples, rate) = crate::audio::load_wav_file(
            reference.to_str().context("reference path must be UTF-8")?,
        )?;
        let samples = if rate == RATE {
            samples
        } else {
            crate::audio::resample(&samples, rate, RATE)?
        };
        if samples.is_empty() || samples.iter().any(|v| !v.is_finite()) {
            bail!("reference audio must be finite and nonempty");
        }
        let length = i32::try_from(samples.len()).context("reference audio too long")?;
        let status =
            unsafe { (self.api.extract)(self.context, samples.as_ptr(), length, &mut self.voice) };
        if status != 0 {
            bail!("libqwen reference extraction: {}", error_text(&self.api));
        }
        Ok(())
    }
    fn params(&self, text: &CString, sampling: &Sampling) -> Result<Params> {
        let max =
            i32::try_from(sampling.max_new_tokens).context("frame cap exceeds native range")?;
        let (transcript, instruct, seed) = self.conditioning.fields();
        Ok(Params {
            abi: ABI,
            text: text.as_ptr(),
            lang: self.language.as_ptr(),
            instruct,
            speaker: ptr::null(),
            reference: ptr::null(),
            reference_len: 0,
            transcript,
            seed,
            max_tokens: max,
            temperature: sampling.temperature as f32,
            top_k: i32::try_from(sampling.top_k)?,
            top_p: 1.0,
            penalty: match &self.conditioning {
                Conditioning::Reference(_) | Conditioning::Frozen { .. } => 1.5,
                Conditioning::Description { .. } => 1.05,
            },
            sub_temperature: sampling.temperature as f32,
            sub_top_k: i32::try_from(sampling.top_k)?,
            sub_top_p: 1.0,
            dump: ptr::null(),
            cancel: None,
            cancel_user: ptr::null_mut(),
            chunk: None,
            chunk_user: ptr::null_mut(),
            embedding: match &self.conditioning {
                Conditioning::Frozen { values, .. } => values.as_ptr(),
                _ => self.voice.embedding,
            },
            embedding_dim: match &self.conditioning {
                Conditioning::Frozen { values, .. } => i32::try_from(values.len())?,
                _ => self.voice.dim,
            },
            codes: self.voice.codes,
            reference_frames: self.voice.frames,
        })
    }
    pub(crate) fn synthesize<F, C>(
        &self,
        text: &str,
        sampling: &Sampling,
        audio: F,
        cancelled: C,
    ) -> Result<Stop>
    where
        F: FnMut(&[f32], u32) -> bool + Send,
        C: Fn() -> bool + Send + Sync,
    {
        let text = ctext(text)?;
        let max =
            i32::try_from(sampling.max_new_tokens).context("frame cap exceeds native range")?;
        let mut state = Callback {
            audio,
            cancelled,
            samples: 0,
            failed: None,
            stopped: false,
        };
        let user = (&mut state as *mut Callback<F, C>).cast::<c_void>();
        let mut params = self.params(&text, sampling)?;
        params.cancel = Some(cancel::<F, C>);
        params.cancel_user = user;
        params.chunk = Some(chunk::<F, C>);
        params.chunk_user = user;
        let mut output = Audio {
            samples: ptr::null_mut(),
            count: 0,
            rate: 0,
            channels: 0,
        };
        // Pinned ABI: blocking call, serial callbacks on its compute thread, no retained user pointer.
        let status = unsafe { (self.api.synthesize)(self.context, &params, &mut output) };
        unsafe {
            (self.api.free_audio)(&mut output);
        }
        if let Some(message) = state.failed {
            bail!("{message}");
        }
        if status == -5 || state.stopped {
            return Ok(Stop::Cancelled);
        }
        if status != 0 {
            bail!("libqwen synthesis: {}", error_text(&self.api));
        }
        if state.samples == 0 {
            bail!("libqwen completed without audio");
        }
        // In the pinned stream every emitted frame has HOP samples; success ends only at EOS or cap.
        // Treat reaching the cap conservatively as truncation, never silently successful partial audio.
        if state.samples >= max as usize * HOP {
            Ok(Stop::CapReached)
        } else {
            Ok(Stop::Eos)
        }
    }
}
fn native_scope(model: &Path, codec: &Path, dimension: usize) -> Result<ConditioningScope> {
    Ok(ConditioningScope {
        backend: ConditioningBackend::Ggml,
        model_sha256: hash_file(model)?,
        codec_sha256: hash_file(codec)?,
        encoder: format!("qwentts.cpp/{REVISION}/raw-speaker-f32"),
        dimension,
        representation: "raw-f32".into(),
    })
}
struct Callback<F, C> {
    audio: F,
    cancelled: C,
    samples: usize,
    failed: Option<&'static str>,
    stopped: bool,
}
extern "C" fn cancel<F, C>(user: *mut c_void) -> bool
where
    F: FnMut(&[f32], u32) -> bool + Send,
    C: Fn() -> bool + Send + Sync,
{
    let state = unsafe { &mut *user.cast::<Callback<F, C>>() };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| (state.cancelled)())) {
        Ok(value) => {
            state.stopped |= value;
            value
        }
        Err(_) => {
            state.failed = Some("cancellation callback panicked");
            true
        }
    }
}
extern "C" fn chunk<F, C>(samples: *const f32, count: i32, user: *mut c_void) -> bool
where
    F: FnMut(&[f32], u32) -> bool + Send,
    C: Fn() -> bool + Send + Sync,
{
    let state = unsafe { &mut *user.cast::<Callback<F, C>>() };
    if count == 0 {
        return true;
    }
    if samples.is_null() || !(1..=192000).contains(&count) {
        state.failed = Some("invalid native audio chunk");
        return false;
    }
    let samples = unsafe { std::slice::from_raw_parts(samples, count as usize) };
    if samples.iter().any(|v| !v.is_finite()) {
        state.failed = Some("non-finite native audio");
        return false;
    }
    state.samples += samples.len();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        (state.audio)(samples, RATE)
    })) {
        Ok(keep) => {
            state.stopped |= !keep;
            keep
        }
        Err(_) => {
            state.failed = Some("audio callback panicked");
            false
        }
    }
}

#[cfg(test)]
mod conditioning_tests {
    use super::*;
    #[test]
    fn design_and_reference_modes_have_disjoint_native_arguments() {
        let design = Conditioning::Description {
            instruction: ctext("Fictional adult German voice").unwrap(),
            seed: 4242,
        };
        assert!(design.check_model_type("voice_design").is_ok());
        for wrong in ["base", "custom_voice", "", "VoiceDesign"] {
            assert!(design.check_model_type(wrong).is_err());
        }
        let (reference, instruction, seed) = design.fields();
        assert!(reference.is_null());
        assert_eq!(
            unsafe { CStr::from_ptr(instruction) }.to_str().unwrap(),
            "Fictional adult German voice"
        );
        assert_eq!(seed, 4242);
        let cloned = Conditioning::Reference(ctext("Matching transcript").unwrap());
        assert!(cloned.check_model_type("base").is_ok());
        assert!(cloned.check_model_type("voice_design").is_err());
        let (reference, instruction, seed) = cloned.fields();
        assert!(!reference.is_null());
        assert!(instruction.is_null());
        assert_eq!(seed, -1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn abi_64_bit_layout_matches_pinned_c_header() {
        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(std::mem::size_of::<Init>(), 48);
            assert_eq!(std::mem::size_of::<Params>(), 176);
            assert_eq!(std::mem::size_of::<Voice>(), 32);
            assert_eq!(std::mem::size_of::<Audio>(), 24);
        }
    }
    fn callback_state<F, C>(audio: F, cancelled: C) -> Callback<F, C>
    where
        F: FnMut(&[f32], u32) -> bool + Send,
        C: Fn() -> bool + Send + Sync,
    {
        let mut state = Callback {
            audio,
            cancelled,
            samples: 0,
            failed: None,
            stopped: false,
        };
        let user = (&mut state as *mut Callback<F, C>).cast();
        if !cancel::<F, C>(user) {
            chunk::<F, C>([0.01].as_ptr(), 1, user);
        }
        state
    }
    #[test]
    fn callback_stop_and_panics_do_not_cross_c_abi() {
        assert!(callback_state(|_, _| false, || false).stopped);
        assert!(callback_state(|_, _| true, || true).stopped);
        assert!(
            callback_state(|_, _| panic!("synthetic callback panic"), || false)
                .failed
                .is_some()
        );
        assert!(
            callback_state(|_, _| true, || panic!("synthetic cancel panic"))
                .failed
                .is_some()
        );
    }
    #[test]
    fn explicit_assets_fail_closed() {
        assert!(assets(Path::new("missing.gguf"), None).is_err());
    }
    #[test]
    fn callbacks_ignore_empty_but_reject_invalid_audio() {
        let mut state = Callback {
            audio: |_: &[f32], _: u32| true,
            cancelled: || false,
            samples: 0,
            failed: None,
            stopped: false,
        };
        fn exercise<F, C>(state: &mut Callback<F, C>)
        where
            F: FnMut(&[f32], u32) -> bool + Send,
            C: Fn() -> bool + Send + Sync,
        {
            let ptr = (state as *mut Callback<F, C>).cast();
            assert!(chunk::<F, C>(ptr::null(), 0, ptr));
            let samples = [f32::NAN];
            assert!(!chunk::<F, C>(samples.as_ptr(), 1, ptr));
            assert!(state.failed.is_some());
        }
        exercise(&mut state);
    }
}
