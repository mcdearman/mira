//! How long the graphics card takes over each pass of a frame.
//!
//! What the engine times on the processor is its own work: gathering a frame, recording it
//! and handing it over. The card draws it afterwards, and how long that takes shows on the
//! processor only once the card has fallen behind and is waited for. So the card is asked:
//! it stamps the time as it comes to the start of the frame's work and to the end of each
//! pass, and the stamps are read back a few frames later, when it has got there.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use super::Gpu;

/// The most passes of a frame that are timed.
const MOST: u32 = 63;
const STAMP: u64 = std::mem::size_of::<u64>() as u64;

struct Stamps {
    set: wgpu::QuerySet,
    resolved: wgpu::Buffer,
    read: wgpu::Buffer,
    /// Nanoseconds to a tick of the card's clock.
    period: f64,
}

/// A frame whose stamps have been asked for, and whether the card has given them.
struct Asked {
    passes: Vec<&'static str>,
    given: Arc<AtomicBool>,
}

/// Times the passes on the card, one frame at a time: the next frame is stamped once the
/// last one's stamps are back.
pub(crate) struct CardClock {
    /// None where the card cannot stamp the time between passes.
    stamps: Option<Stamps>,
    /// The passes stamped so far in the frame being recorded, if this one is stamped.
    stamping: Option<Vec<&'static str>>,
    asked: Option<Asked>,
}

impl CardClock {
    pub(crate) fn new(gpu: &Gpu) -> Self {
        let wanted = wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
        let stamps = gpu.device.features().contains(wanted).then(|| {
            let size = (MOST as u64 + 1) * STAMP;
            let buffer = |label, usage| gpu.device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: false });
            Stamps {
                set: gpu.device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("pass times"), ty: wgpu::QueryType::Timestamp, count: MOST + 1 }),
                resolved: buffer("pass times", wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC),
                read: buffer("pass times read back", wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST),
                period: gpu.queue.get_timestamp_period() as f64,
            }
        });
        Self { stamps, stamping: None, asked: None }
    }

    /// What the card took over each pass of the last frame it has answered for, once, when
    /// the answer comes.
    pub(crate) fn answer(&mut self) -> Option<Vec<(&'static str, Duration)>> {
        let stamps = self.stamps.as_ref()?;
        if !self.asked.as_ref()?.given.load(Ordering::Acquire) {
            return None;
        }
        let asked = self.asked.take()?;
        let times = stamps.read.slice(..).get_mapped_range().ok().map(|bytes| {
            let ticks: &[u64] = bytemuck::cast_slice(&bytes);
            asked
                .passes
                .iter()
                .enumerate()
                // A clock that went backwards says nothing of the pass.
                .map(|(i, name)| (*name, Duration::from_nanos((ticks[i + 1].saturating_sub(ticks[i]) as f64 * stamps.period) as u64)))
                .collect()
        });
        stamps.read.unmap();
        times
    }

    /// Before the first pass records: stamps the start of the frame, unless the last frame
    /// stamped is still to be answered for.
    pub(crate) fn begin(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let Some(stamps) = self.stamps.as_ref().filter(|_| self.asked.is_none()) else { return };
        encoder.write_timestamp(&stamps.set, 0);
        self.stamping = Some(Vec::new());
    }

    /// After a pass has recorded: stamps its end.
    pub(crate) fn passed(&mut self, encoder: &mut wgpu::CommandEncoder, pass: &'static str) {
        let (Some(stamps), Some(passes)) = (&self.stamps, &mut self.stamping) else { return };
        if passes.len() < MOST as usize {
            passes.push(pass);
            encoder.write_timestamp(&stamps.set, passes.len() as u32);
        }
    }

    /// After the last pass: records the reading of the stamps, in work that reaches the card
    /// after the passes'.
    pub(crate) fn read_back(&mut self, encoder: &mut wgpu::CommandEncoder) {
        let (Some(stamps), Some(passes)) = (&self.stamps, &self.stamping) else { return };
        let count = passes.len() as u32 + 1;
        encoder.resolve_query_set(&stamps.set, 0..count, &stamps.resolved, 0);
        encoder.copy_buffer_to_buffer(&stamps.resolved, 0, &stamps.read, 0, count as u64 * STAMP);
    }

    /// Once that work is sent: asks for the stamps, which come when the card has done it.
    pub(crate) fn sent(&mut self) {
        let (Some(stamps), Some(passes)) = (&self.stamps, self.stamping.take()) else { return };
        let given = Arc::new(AtomicBool::new(false));
        let told = given.clone();
        stamps.read.slice(..).map_async(wgpu::MapMode::Read, move |read| {
            if read.is_ok() {
                told.store(true, Ordering::Release);
            }
        });
        self.asked = Some(Asked { passes, given });
    }
}
