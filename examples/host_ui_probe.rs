//! Main-thread host request/FFI regression; uses original pixels, no game assets.
use std::path::PathBuf;
use wire::host::ui::{Frame, Presenter, WindowRequest};

fn main() -> Result<(), String> {
    let path = PathBuf::from(
        std::env::args()
            .nth(1)
            .ok_or("usage: host_ui_probe CAPTURE_DIR")?,
    );
    let off_thread = std::thread::spawn(|| Presenter::new(true, None).err())
        .join()
        .unwrap();
    assert_eq!(
        off_thread.as_deref(),
        Some("AppKit requires the main thread")
    );
    let mut ui = Presenter::new(true, Some(path.clone()))?;
    assert!(ui.accept_frame(1));
    ui.submit(Frame::new(1, 2, 1, vec![11, 22, 33, 255, 44, 55, 66, 255])?);
    assert_eq!(ui.displayed, 0);
    assert_eq!(
        ui.error.take().as_deref(),
        Some("AppKit frame upload failed")
    );
    for (sequence, size, red) in [(2, 128, 11), (3, 192, 91)] {
        ui.sync_windows(vec![WindowRequest {
            id: 1,
            title: "wire host request validation\0".into(),
            width: size,
            height: 64,
            visible: true,
        }]);
        for _ in 0..512 {
            if ui.next_event().is_none() {
                break;
            }
        }
        assert!(ui.accept_frame(sequence));
        ui.submit(Frame::new(
            1,
            2,
            1,
            vec![red, 22, 33, 255, 44, 55, 66, 255],
        )?);
        assert_eq!(ui.displayed, sequence - 1);
        assert!(ui.error.is_none());
    }
    assert!(!path.join("frame-000002.ppm").exists());
    assert_eq!(
        std::fs::read(path.join("frame-000003.ppm")).map_err(|e| e.to_string())?,
        b"P6\n2 1\n255\n\x5b\x16\x21\x2c\x37\x42"
    );
    ui.sync_windows(Vec::new());
    assert!(ui.accept_frame(4));
    ui.submit(Frame::new(1, 1, 1, vec![91, 22, 33, 255])?);
    assert_eq!(ui.displayed, 2);
    assert_eq!(ui.error.as_deref(), Some("AppKit frame upload failed"));
    drop(ui);
    println!("host UI passed: main-thread constraint, upload/failure, resize/removal, captures");
    Ok(())
}
