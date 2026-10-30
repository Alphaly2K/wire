//! DirectSound shares COM and PCM semantics; the host supplies a PCM consumer.
use super::{dsound_core, heaps, objects, user, ServiceError, Services};
use crate::{
    memory::{Fault, Memory, READ, WRITE},
    process::Loaded,
};
pub(crate) use dsound_core::Method;
enum Failure {
    Hresult(u32),
    Service(ServiceError),
}
impl From<u32> for Failure {
    fn from(value: u32) -> Self {
        Self::Hresult(value)
    }
}
impl From<Fault> for Failure {
    fn from(value: Fault) -> Self {
        Self::Service(value.into())
    }
}
fn heap_error(error: heaps::Error) -> Failure {
    match error {
        heaps::Error::Windows(8) => Failure::Hresult(0x8007000e),
        heaps::Error::Fault(fault) => fault.into(),
        _ => Failure::Service(ServiceError::Unavailable(
            "DirectSound process allocation was lost",
        )),
    }
}
struct PcmMemory<'a>(&'a mut Memory);
impl dsound_core::Memory for PcmMemory<'_> {
    type Error = Failure;
    fn check(&self, p: u32, n: u32, write: bool) -> Result<(), Failure> {
        if p == 0 {
            return Err(0x80070057.into());
        }
        Ok(self
            .0
            .check(u64::from(p), n as usize, if write { WRITE } else { READ })?)
    }
    fn read_bytes(&self, p: u32, b: &mut [u8]) -> Result<(), Failure> {
        self.check(p, b.len() as u32, false)?;
        Ok(self.0.read(u64::from(p), b)?)
    }
    fn write_bytes(&mut self, p: u32, b: &[u8]) -> Result<(), Failure> {
        self.check(p, b.len() as u32, true)?;
        Ok(self.0.write(u64::from(p), b)?)
    }
}
struct Access<'a> {
    memory: PcmMemory<'a>,
    heaps: &'a mut heaps::Heaps,
    services: &'a mut Services,
    objects: &'a objects::Objects,
    user: &'a user::User,
    enabled: bool,
}
impl dsound_core::Memory for Access<'_> {
    type Error = Failure;
    fn check(&self, p: u32, n: u32, w: bool) -> Result<(), Failure> {
        self.memory.check(p, n, w)
    }
    fn read_bytes(&self, p: u32, b: &mut [u8]) -> Result<(), Failure> {
        self.memory.read_bytes(p, b)
    }
    fn write_bytes(&mut self, p: u32, b: &[u8]) -> Result<(), Failure> {
        self.memory.write_bytes(p, b)
    }
}
impl dsound_core::Backend for Access<'_> {
    fn allocate(&mut self, n: u32) -> Result<u32, Failure> {
        self.heaps
            .allocate_local(self.memory.0, u64::from(n), &[])
            .map(|p| p as u32)
            .map_err(heap_error)
    }
    fn check_free(&self, p: u32) -> Result<(), Failure> {
        self.heaps
            .check_process_free(self.memory.0, u64::from(p))
            .map_err(heap_error)
    }
    fn free(&mut self, p: u32) -> Result<(), Failure> {
        self.heaps
            .free_process(self.memory.0, u64::from(p))
            .map_err(heap_error)
    }
    fn check_methods(&self, kind: &str) -> Result<(), Failure> {
        self.services
            .check_sound_methods(kind)
            .map_err(|_| 0x8007000e.into())
    }
    fn register(&mut self, m: Method) -> Result<u32, Failure> {
        self.services
            .register_sound(m)
            .map(|p| p as u32)
            .map_err(|_| 0x8007000e.into())
    }
    fn event(&self, h: u32) -> bool {
        self.objects.pulse_identity(u64::from(h)).is_ok()
    }
    fn window(&self, h: u32) -> bool {
        self.user.windows.contains_key(&u64::from(h))
    }
    fn device(&mut self) -> bool {
        self.enabled
    }
    fn status(error: &Failure) -> Option<u32> {
        if let Failure::Hresult(code) = error {
            Some(*code)
        } else {
            None
        }
    }
}
#[derive(Default)]
pub(crate) struct Owner {
    sound: dsound_core::DSound,
    pub(crate) enabled: bool,
    last: Option<u64>,
    fraction: u64,
    pub(crate) pcm: Vec<i16>,
    pub(crate) dropped_frames: u64,
}
fn signal(sound: &mut dsound_core::DSound, objects: &mut objects::Objects) {
    for event in sound.take_events() {
        let _ = objects.change(u64::from(event), true);
    }
}
impl Owner {
    pub(crate) fn stat(&self, index: u32) -> u64 {
        match index {
            0 => self.sound.mixed_frames,
            1 => self.dropped_frames,
            2 => self.sound.locked_playback_frames,
            _ => 0,
        }
    }
    pub(crate) fn enable(&mut self, now: u64) {
        self.enabled = true;
        self.last = Some(now);
    }
    pub(crate) fn pump(
        &mut self,
        memory: &mut Memory,
        objects: &mut objects::Objects,
        now: u64,
    ) -> Result<(), ServiceError> {
        if !self.enabled || !self.pcm.is_empty() {
            return Ok(());
        }
        if !self.sound.playing() {
            self.last = Some(now);
            self.fraction = 0;
            return Ok(());
        }
        let last = self.last.unwrap_or(now);
        let delta = now
            .checked_sub(last)
            .ok_or(ServiceError::Unavailable("audio clock moved backwards"))?;
        if delta < 50_000 {
            return Ok(());
        }
        let total = u128::from(delta) * 44100 + u128::from(self.fraction);
        let frames = (total / 10_000_000).min(44100) as u32;
        let samples = self
            .sound
            .mix(&PcmMemory(memory), frames)
            .map_err(|error| match error {
                Failure::Service(error) => error,
                Failure::Hresult(_) => ServiceError::Unavailable("invalid PCM backing"),
            })?;
        self.last = Some(now);
        self.fraction = (total % 10_000_000) as u64;
        self.dropped_frames = self.dropped_frames.saturating_add(
            (total / 10_000_000)
                .saturating_sub(u128::from(frames))
                .min(u128::from(u64::MAX)) as u64,
        );
        self.pcm = samples;
        signal(&mut self.sound, objects);
        Ok(())
    }
}
pub(super) fn dispatch(
    method: Method,
    arguments: &[u64; 16],
    memory: &mut Memory,
    image: &mut Loaded,
    services: &mut Services,
) -> Result<u64, ServiceError> {
    let args = arguments.map(|n| n as u32);
    let result = image.audio.sound.call(
        method.name,
        &args,
        &mut Access {
            memory: PcmMemory(memory),
            heaps: &mut image.heaps,
            services,
            objects: &image.objects,
            user: &image.user,
            enabled: image.audio.enabled,
        },
    );
    match result {
        Ok(value) | Err(Failure::Hresult(value)) => {
            signal(&mut image.audio.sound, &mut image.objects);
            Ok(u64::from(value))
        }
        Err(Failure::Service(error)) => Err(error),
    }
}
