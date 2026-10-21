//! Owned native UI requests. This layer knows no guest memory, CPU or Win32 state.
use std::{
    collections::BTreeSet,
    ffi::CString,
    path::PathBuf,
    time::{Duration, Instant},
};

pub(crate) fn trace_input(stage: &str, window: u64, message: u32, wp: u64, lp: u64, tid: u64) {
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        OnceLock,
    };
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static EVENTS: AtomicU32 = AtomicU32::new(0);
    static MOVES: AtomicU32 = AtomicU32::new(0);
    if !*ENABLED.get_or_init(|| std::env::var_os("WIRE_INPUT_TRACE").is_some()) {
        return;
    }
    let counter = if message == 0x200 { &MOVES } else { &EVENTS };
    let limit = if message == 0x200 { 512 } else { 4096 };
    if counter.fetch_add(1, Ordering::Relaxed) < limit {
        eprintln!(
            "input {stage}: HWND={window:#x} msg={message:#x} wp={wp:#x} lp={lp:#x} tid={tid}"
        );
    }
}
#[repr(C)]
#[derive(Default)]
pub(crate) struct Event {
    pub hwnd: u32,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
}
#[cfg(target_os = "macos")]
extern "C" {
    pub(crate) fn wire_ui_init() -> i32;
    pub(crate) fn wire_ui_window(
        hwnd: u32,
        title: *const std::ffi::c_char,
        width: u32,
        height: u32,
        visible: i32,
    );
    pub(crate) fn wire_ui_remove(hwnd: u32);
    pub(crate) fn wire_ui_window_topmost(hwnd: u32, topmost: i32);
    fn wire_ui_metal_layer(hwnd: u32) -> *mut std::ffi::c_void;
    fn wire_ui_release_metal_layer(layer: *mut std::ffi::c_void);
    fn wire_ui_frame(hwnd: u32, rgba: *const u8, width: u32, height: u32) -> i32;
    pub(crate) fn wire_ui_poll(event: *mut Event) -> i32;
    pub(crate) fn wire_ui_shutdown();
}
/// A retained platform layer. This pointer is confined to the native bridge.
#[derive(Clone)]
pub(crate) struct MetalLayer(std::rc::Rc<LayerInner>);
struct LayerInner(std::ptr::NonNull<std::ffi::c_void>);
impl MetalLayer {
    #[cfg(target_os = "macos")]
    pub(crate) fn for_window(hwnd: u32) -> Result<Self, String> {
        let layer = std::ptr::NonNull::new(unsafe { wire_ui_metal_layer(hwnd) })
            .ok_or("no visible native window for Metal surface")?;
        Ok(Self(std::rc::Rc::new(LayerInner(layer))))
    }
    pub(crate) fn as_ptr(&self) -> *mut std::ffi::c_void {
        self.0 .0.as_ptr()
    }
}
impl Drop for LayerInner {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        unsafe {
            wire_ui_release_metal_layer(self.0.as_ptr())
        };
    }
}

/// A native window ID is supplied by the architecture's guest adapter.
pub struct WindowRequest {
    pub id: u32,
    pub title: String,
    pub width: u32,
    pub height: u32,
    pub visible: bool,
}

#[cfg(target_os = "macos")]
pub(crate) fn update_window(window: WindowRequest) -> Result<(), String> {
    let title = CString::new(window.title.replace('\0', "")).map_err(|e| e.to_string())?;
    unsafe {
        wire_ui_window(
            window.id,
            title.as_ptr(),
            window.width,
            window.height,
            window.visible as i32,
        );
    }
    Ok(())
}

#[cfg(target_os = "macos")]
pub(crate) fn remove_window(id: u32) {
    unsafe { wire_ui_remove(id) }
}

#[cfg(target_os = "macos")]
pub(crate) fn set_topmost(id: u32, topmost: bool) {
    unsafe { wire_ui_window_topmost(id, topmost as i32) }
}

#[cfg(target_os = "macos")]
pub(crate) fn poll_event() -> Option<WindowEvent> {
    let mut event = Event::default();
    (unsafe { wire_ui_poll(&mut event) } != 0).then_some(WindowEvent {
        window: event.hwnd,
        message: event.message,
        wparam: event.wparam,
        lparam: event.lparam,
    })
}

/// Owned top-down RGBA bytes; dimensions are checked before crossing FFI.
pub struct Frame {
    window: u32,
    width: u32,
    height: u32,
    rgba: Vec<u8>,
}
impl Frame {
    pub fn new(window: u32, width: u32, height: u32, rgba: Vec<u8>) -> Result<Self, String> {
        let length = (width as usize)
            .checked_mul(height as usize)
            .and_then(|n| n.checked_mul(4))
            .ok_or("invalid host RGBA frame dimensions")?;
        if width == 0 || height == 0 || rgba.len() != length {
            return Err("invalid host RGBA frame dimensions".into());
        }
        Ok(Self {
            window,
            width,
            height,
            rgba,
        })
    }
}
pub struct Presenter {
    // AppKit resources stay on the thread that passed main-thread initialization.
    thread: std::marker::PhantomData<std::rc::Rc<()>>,
    enabled: bool,
    next_poll: Instant,
    windows: BTreeSet<u32>,
    frame: u64,
    capture: Option<PathBuf>,
    captures: u32,
    last_hash: u64,
    pub displayed: u64,
    pub error: Option<String>,
}
impl Default for Presenter {
    fn default() -> Self {
        Self {
            thread: std::marker::PhantomData,
            enabled: false,
            next_poll: Instant::now(),
            windows: BTreeSet::new(),
            frame: 0,
            capture: None,
            captures: 0,
            last_hash: 0,
            displayed: 0,
            error: None,
        }
    }
}

impl Presenter {
    pub fn new(enabled: bool, capture: Option<PathBuf>) -> Result<Self, String> {
        if let Some(p) = &capture {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "macos")]
        if enabled && unsafe { wire_ui_init() } == 0 {
            return Err("AppKit requires the main thread".into());
        }
        #[cfg(not(target_os = "macos"))]
        if enabled {
            return Err("visible presenter currently requires macOS; use --headless".into());
        }
        let mut p = Self::default();
        p.enabled = enabled;
        p.capture = capture;
        Ok(p)
    }
    pub(crate) fn active(&self) -> bool {
        self.enabled
    }

    pub fn poll_due(&mut self) -> bool {
        if !self.enabled && self.capture.is_none() {
            return false;
        }
        let now = Instant::now();
        if now < self.next_poll {
            return false;
        }
        self.next_poll = now + Duration::from_millis(16);
        true
    }

    /// Update native windows before consuming events at the scheduling boundary.
    pub fn sync_windows(&mut self, requests: Vec<WindowRequest>) {
        let live: BTreeSet<_> = requests.iter().map(|w| w.id).collect();
        #[cfg(target_os = "macos")]
        if self.enabled {
            for &id in self.windows.difference(&live) {
                remove_window(id);
            }
            for window in requests {
                update_window(window).unwrap();
            }
        }
        self.windows = live;
    }

    /// The adapter queues this event; native UI callbacks never run guest code.
    pub fn next_event(&mut self) -> Option<WindowEvent> {
        #[cfg(target_os = "macos")]
        if self.enabled {
            return poll_event();
        }
        None
    }

    /// Consume the frame sequence before snapshotting, including failed snapshots.
    pub fn accept_frame(&mut self, sequence: u64) -> bool {
        if sequence == self.frame {
            return false;
        }
        self.frame = sequence;
        true
    }

    pub fn submit(&mut self, frame: Frame) {
        let Frame {
            window,
            width: w,
            height: h,
            rgba,
        } = frame;
        #[cfg(target_os = "macos")]
        if self.enabled {
            if unsafe { wire_ui_frame(window, rgba.as_ptr(), w, h) } != 0 {
                self.displayed += 1;
            } else {
                self.error = Some("AppKit frame upload failed".into());
            }
        }
        if let Some(p) = self.capture.as_ref().filter(|_| self.captures < 16) {
            let hash = rgba.iter().fold(0xcbf29ce484222325u64, |n, &b| {
                (n ^ b as u64).wrapping_mul(0x100000001b3)
            });
            if self.captures == 0 || hash != self.last_hash {
                let mut ppm = format!("P6\n{w} {h}\n255\n").into_bytes();
                for c in rgba.as_chunks::<4>().0 {
                    ppm.extend_from_slice(&c[..3]);
                }
                let path = p.join(format!("frame-{:06}.ppm", self.frame));
                if let Err(e) = std::fs::write(&path, ppm) {
                    self.error = Some(format!("capture {}: {e}", path.display()));
                } else {
                    self.captures += 1;
                    self.last_hash = hash;
                }
            }
        }
    }
}
/// Native mouse coordinates are signed client pixels, including wheel events.
/// The guest adapter maps window IDs and converts wheel points to guest screen pixels.
pub struct WindowEvent {
    pub window: u32,
    pub message: u32,
    pub wparam: u32,
    pub lparam: u32,
}
impl Drop for Presenter {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.enabled {
            unsafe { wire_ui_shutdown() }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Frame, Presenter};

    #[test]
    fn malformed_owned_frames_are_rejected_before_native_ffi() {
        for (width, height, bytes) in [
            (0, 2, 0),
            (2, 0, 0),
            (2, 2, 15),
            (2, 2, 17),
            (u32::MAX, u32::MAX, 0),
        ] {
            assert!(Frame::new(1, width, height, vec![0; bytes]).is_err());
        }
        assert!(Frame::new(1, 2, 2, vec![0; 16]).is_ok());
    }

    #[test]
    fn captures_keep_rgb_bytes_deduplication_and_the_sixteen_frame_limit() {
        let path = std::env::temp_dir().join(format!("wire-host-ui-{}", std::process::id()));
        let mut presenter = Presenter::new(false, Some(path.clone())).unwrap();
        assert!(presenter.accept_frame(1));
        presenter.submit(Frame::new(9, 2, 1, vec![11, 22, 33, 255, 44, 55, 66, 255]).unwrap());
        assert_eq!(
            std::fs::read(path.join("frame-000001.ppm")).unwrap(),
            b"P6\n2 1\n255\n\x0b\x16\x21\x2c\x37\x42"
        );
        assert!(!presenter.accept_frame(1));
        assert!(presenter.accept_frame(2));
        presenter.submit(Frame::new(9, 2, 1, vec![11, 22, 33, 255, 44, 55, 66, 255]).unwrap());
        assert!(!path.join("frame-000002.ppm").exists());
        for sequence in 3..=20 {
            assert!(presenter.accept_frame(sequence));
            presenter.submit(Frame::new(9, 1, 1, vec![sequence as u8, 0, 0, 255]).unwrap());
        }
        assert_eq!(std::fs::read_dir(&path).unwrap().count(), 16);
        assert_eq!(presenter.displayed, 0);
        assert!(presenter.error.is_none());
        drop(presenter);
        std::fs::remove_dir_all(path).unwrap();
    }
}
