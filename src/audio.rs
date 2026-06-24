//! Cattura audio tramite PipeWire.
//!
//! Catturiamo in **stereo** (2 canali) il monitor del sink di default (output)
//! oppure la sorgente di default (input). Il thread PipeWire scrive i campioni
//! `f32` dei due canali in un ring buffer condiviso, letto dal modulo DSP che
//! calcola due spettri (sinistro/destro) per la visualizzazione speculare.

use crate::config::AudioSource;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Frequenza di campionamento nominale usata dal binning DSP.
/// (Il grafo PipeWire gira tipicamente a 48 kHz.)
pub const SAMPLE_RATE: u32 = 48_000;
/// Canali catturati (stereo).
const CHANNELS: u32 = 2;

/// Canale audio da analizzare.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    Left,
    Right,
}

/// Contenuto del ring buffer: i due canali separati.
struct Stereo {
    left: VecDeque<f32>,
    right: VecDeque<f32>,
}

/// Ring buffer condiviso, stereo.
pub struct AudioBuffer {
    inner: Mutex<Stereo>,
    capacity: usize,
}

impl AudioBuffer {
    /// Crea un buffer in grado di contenere `capacity` campioni per canale.
    pub fn new(capacity: usize) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Stereo {
                left: VecDeque::with_capacity(capacity),
                right: VecDeque::with_capacity(capacity),
            }),
            capacity,
        })
    }

    /// Aggiunge un blocco di campioni interleaved (`n_channels` per frame),
    /// separando i canali sinistro e destro e mantenendo la capacità massima.
    fn push_interleaved(&self, data: &[f32], n_channels: usize) {
        if n_channels == 0 {
            return;
        }
        let frames = data.len() / n_channels;
        let mut buf = self.inner.lock().unwrap();
        for f in 0..frames {
            let base = f * n_channels;
            let l = data[base];
            // Mono in arrivo (1 canale) → destro = sinistro.
            let r = if n_channels >= 2 { data[base + 1] } else { l };
            buf.left.push_back(l);
            buf.right.push_back(r);
        }
        let overflow = buf.left.len().saturating_sub(self.capacity);
        if overflow > 0 {
            buf.left.drain(0..overflow);
            buf.right.drain(0..overflow);
        }
    }

    /// Copia gli ultimi `out.len()` campioni del canale richiesto in `out`,
    /// riempiendo con zeri a sinistra se non ce ne sono abbastanza.
    pub fn snapshot(&self, channel: Channel, out: &mut [f32]) {
        let buf = self.inner.lock().unwrap();
        let src = match channel {
            Channel::Left => &buf.left,
            Channel::Right => &buf.right,
        };
        let n = out.len();
        let available = src.len();
        if available >= n {
            let start = available - n;
            for (i, s) in src.iter().skip(start).enumerate() {
                out[i] = *s;
            }
        } else {
            let pad = n - available;
            out[..pad].iter_mut().for_each(|x| *x = 0.0);
            for (i, s) in src.iter().enumerate() {
                out[pad + i] = *s;
            }
        }
    }
}

/// Comando inviato al loop PipeWire dal thread UI.
enum AudioCommand {
    Stop,
}

/// Handle a una sessione di cattura attiva. Permette lo stop/switch a runtime.
pub struct AudioHandle {
    sender: pipewire::channel::Sender<AudioCommand>,
    join: Option<std::thread::JoinHandle<()>>,
}

impl AudioHandle {
    /// Ferma la cattura corrente e attende la chiusura del thread.
    pub fn stop(mut self) {
        let _ = self.sender.send(AudioCommand::Stop);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Avvia la cattura audio su un thread PipeWire dedicato e ritorna un handle
/// per fermarla (necessario per lo switch sorgente a runtime).
pub fn start(buffer: Arc<AudioBuffer>, source: AudioSource) -> AudioHandle {
    let (sender, receiver) = pipewire::channel::channel::<AudioCommand>();
    let join = std::thread::Builder::new()
        .name("sinestesia-audio".into())
        .spawn(move || {
            if let Err(e) = run(buffer, source, receiver) {
                log::error!("thread audio terminato con errore: {e}");
            }
        })
        .expect("impossibile avviare il thread audio");
    AudioHandle {
        sender,
        join: Some(join),
    }
}

/// Dati condivisi col listener PipeWire: formato negoziato + ring buffer.
struct UserData {
    format: pipewire::spa::param::audio::AudioInfoRaw,
    buffer: Arc<AudioBuffer>,
}

fn run(
    buffer: Arc<AudioBuffer>,
    source: AudioSource,
    receiver: pipewire::channel::Receiver<AudioCommand>,
) -> anyhow::Result<()> {
    use pipewire as pw;
    use pw::spa;
    use spa::param::format::{MediaSubtype, MediaType};
    use spa::param::format_utils;
    use spa::pod::Pod;

    pw::init();

    let mainloop = pw::main_loop::MainLoopRc::new(None)?;
    let context = pw::context::ContextRc::new(&mainloop, None)?;
    let core = context.connect_rc(None)?;

    // Riceve comandi dal thread UI (es. Stop per switch sorgente).
    let ml = mainloop.clone();
    let _receiver = receiver.attach(mainloop.loop_(), move |cmd| match cmd {
        AudioCommand::Stop => ml.quit(),
    });

    // In modalità Output catturiamo il monitor del sink di default
    // (stream.capture.sink = true). In modalità Input registriamo la
    // sorgente di default (microfono).
    let capture_sink = matches!(source, AudioSource::Output);

    let mut props = pw::properties::properties! {
        *pw::keys::MEDIA_TYPE => "Audio",
        *pw::keys::MEDIA_CATEGORY => "Capture",
        *pw::keys::MEDIA_ROLE => "Music",
        *pw::keys::NODE_NAME => "sinestesia",
    };
    if capture_sink {
        props.insert(*pw::keys::STREAM_CAPTURE_SINK, "true");
    }

    let stream = pw::stream::StreamRc::new(core, "sinestesia-capture", props)?;

    let data = UserData {
        format: Default::default(),
        buffer,
    };

    let _listener = stream
        .add_local_listener_with_user_data(data)
        .param_changed(|_, user_data, id, param| {
            let Some(param) = param else {
                return;
            };
            if id != pw::spa::param::ParamType::Format.as_raw() {
                return;
            }
            let Ok((media_type, media_subtype)) = format_utils::parse_format(param) else {
                return;
            };
            if media_type != MediaType::Audio || media_subtype != MediaSubtype::Raw {
                return;
            }
            if user_data.format.parse(param).is_ok() {
                log::info!(
                    "formato negoziato: {} Hz, {} canali",
                    user_data.format.rate(),
                    user_data.format.channels()
                );
            }
        })
        .process(|stream, user_data| {
            let Some(mut buf) = stream.dequeue_buffer() else {
                return;
            };
            let datas = buf.datas_mut();
            if datas.is_empty() {
                return;
            }
            let n_channels = user_data.format.channels().max(1) as usize;
            let data = &mut datas[0];
            let chunk_size = data.chunk().size() as usize;
            let Some(slice) = data.data() else {
                return;
            };
            let n_floats = (chunk_size / std::mem::size_of::<f32>()).min(slice.len() / 4);
            if n_floats == 0 {
                return;
            }
            let floats =
                unsafe { std::slice::from_raw_parts(slice.as_ptr().cast::<f32>(), n_floats) };

            // Separa i canali (interleaved) nel ring buffer stereo.
            user_data.buffer.push_interleaved(floats, n_channels);
        })
        .register()?;

    // Chiediamo un flusso STEREO F32LE a SAMPLE_RATE: PipeWire adatta i canali
    // (N → 2) tramite il suo convertitore. Per una sorgente mono (microfono)
    // l'upmix produce due canali identici, e il livello di rendering applica
    // comunque il mirror in modalità input.
    let mut audio_info = spa::param::audio::AudioInfoRaw::new();
    audio_info.set_format(spa::param::audio::AudioFormat::F32LE);
    audio_info.set_rate(SAMPLE_RATE);
    audio_info.set_channels(CHANNELS);
    let mut position = [0u32; spa::param::audio::MAX_CHANNELS];
    position[0] = pw::spa::sys::SPA_AUDIO_CHANNEL_FL;
    position[1] = pw::spa::sys::SPA_AUDIO_CHANNEL_FR;
    audio_info.set_position(position);

    let values: Vec<u8> = pw::spa::pod::serialize::PodSerializer::serialize(
        std::io::Cursor::new(Vec::new()),
        &pw::spa::pod::Value::Object(pw::spa::pod::Object {
            type_: pw::spa::utils::SpaTypes::ObjectParamFormat.as_raw(),
            id: pw::spa::param::ParamType::EnumFormat.as_raw(),
            properties: audio_info.into(),
        }),
    )?
    .0
    .into_inner();

    let mut params = [Pod::from_bytes(&values).ok_or_else(|| anyhow::anyhow!("pod non valido"))?];

    stream.connect(
        spa::utils::Direction::Input,
        None,
        pw::stream::StreamFlags::AUTOCONNECT
            | pw::stream::StreamFlags::MAP_BUFFERS
            | pw::stream::StreamFlags::RT_PROCESS,
        &mut params,
    )?;

    log::info!(
        "cattura audio avviata (sorgente: {})",
        if capture_sink { "output/monitor" } else { "input" }
    );
    mainloop.run();
    Ok(())
}
