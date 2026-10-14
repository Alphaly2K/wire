//! Existing behavioral tests grouped by audio; expectations remain independent of API catalogs.

use super::*;

#[test]
fn unsupported_audio_returns_real_failure_and_never_a_success_with_null_object() {
    let mut v = Vm::new();
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA, 1]),
        0x80040110
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
    assert_eq!(v.call("winmm.dll", "waveOutGetNumDevs", &[]), 0);
    v.mem.write_u32(DATA, 0xdeadbeef).unwrap();
    assert_eq!(
        v.call(
            "winmm.dll",
            "waveOutOpen",
            &[DATA, u32::MAX, DATA + 100, 0, 0, 0]
        ),
        6
    );
    assert_eq!(v.mem.read_u32(DATA).unwrap(), 0);
}

#[test]
fn directsound_notify_identity_keeps_buffer_alive_and_stop_wakes_event() {
    let mut v = Vm::new();
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA + 0x900, 0]),
        0
    );
    let sound = v.mem.read_u32(DATA + 0x900).unwrap();
    for (i, n) in [20u32, 0x100, 64, 0, DATA + 0x100].into_iter().enumerate() {
        v.mem.write_u32(DATA + i as u32 * 4, n).unwrap();
    }
    v.mem
        .write_bytes(
            DATA + 0x100,
            &[
                1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, 0, 0,
            ],
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, sound, 3, &[DATA, DATA + 0x910, 0]), 0);
    let buffer = v.mem.read_u32(DATA + 0x910).unwrap();
    let notify_iid = [
        0x83, 7, 0x21, 0xb0, 0xcd, 0x89, 0xd0, 0x11, 0xaf, 8, 0, 0xa0, 0xc9, 0x25, 0xcd, 0x16,
    ];
    v.mem.write_bytes(DATA + 0x300, &notify_iid).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 0, &[DATA + 0x300, DATA + 0x910]), 0);
    let notify = v.mem.read_u32(DATA + 0x910).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 0, &[DATA + 0x300, DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), notify);
    assert_eq!(d3d_com(&mut v, notify, 2, &[]), 1);
    v.mem
        .write_bytes(
            DATA + 0x300,
            &[
                0x85, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0, 0x20, 0xaf, 0xb,
                0xe5, 0x60,
            ],
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, notify, 0, &[DATA + 0x300, DATA + 0x910]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), buffer);
    d3d_com(&mut v, buffer, 2, &[]);
    let event = v.k("CreateEventW", &[0, 0, 0, 0]);
    v.mem.write_u32(DATA + 0x200, u32::MAX).unwrap();
    v.mem.write_u32(DATA + 0x204, event).unwrap();
    assert_eq!(d3d_com(&mut v, notify, 3, &[1, DATA + 0x200]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 12, &[0, 0, 1]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 18, &[]), 0);
    assert_eq!(v.k("WaitForSingleObject", &[event, 0]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 2, &[]), 1);
    assert_ne!(
        v.k("HeapSize", &[v.state.process_heap, 0, buffer]),
        u32::MAX
    );
    assert_eq!(d3d_com(&mut v, notify, 2, &[]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, buffer]),
        u32::MAX
    );
    d3d_com(&mut v, sound, 2, &[]);
}

#[test]
fn directsound_pcm_buffers_check_wrapped_locks_controls_and_release_storage() {
    let mut v = Vm::new();
    assert_eq!(
        v.call("dsound.dll", "DirectSoundCreate", &[0, DATA + 0x900, 0]),
        0
    );
    let sound = v.mem.read_u32(DATA + 0x900).unwrap();
    v.mem.write_u32(DATA, 20).unwrap();
    v.mem.write_u32(DATA + 4, 0xe0).unwrap();
    v.mem.write_u32(DATA + 8, 8).unwrap();
    v.mem.write_u32(DATA + 16, DATA + 0x100).unwrap();
    v.mem
        .write_bytes(
            DATA + 0x100,
            &[
                1, 0, 1, 0, 0x44, 0xac, 0, 0, 0x88, 0x58, 1, 0, 2, 0, 16, 0, 0, 0,
            ],
        )
        .unwrap();
    assert_eq!(d3d_com(&mut v, sound, 3, &[DATA, DATA + 0x910, 0]), 0);
    let buffer = v.mem.read_u32(DATA + 0x910).unwrap();
    assert_ne!(buffer, 0);
    assert_eq!(
        d3d_com(
            &mut v,
            buffer,
            11,
            &[
                6,
                4,
                DATA + 0x920,
                DATA + 0x924,
                DATA + 0x928,
                DATA + 0x92c,
                0
            ]
        ),
        0
    );
    let first = v.mem.read_u32(DATA + 0x920).unwrap();
    let second = v.mem.read_u32(DATA + 0x928).unwrap();
    assert_eq!(first, second + 6);
    assert_eq!(v.mem.read_u32(DATA + 0x924).unwrap(), 2);
    assert_eq!(v.mem.read_u32(DATA + 0x92c).unwrap(), 2);
    assert_eq!(
        d3d_com(&mut v, buffer, 19, &[first, 3, second, 2]),
        0x80070057
    );
    v.mem.write_u16(first, 0x7fff).unwrap();
    v.mem.write_u16(second, 0x8000).unwrap();
    assert_eq!(d3d_com(&mut v, buffer, 19, &[first, 2, second, 2]), 0);
    assert_eq!(
        d3d_com(
            &mut v,
            buffer,
            11,
            &[7, 9, DATA + 0x920, DATA + 0x924, 0, 0, 0]
        ),
        0x80070057
    );
    assert_eq!(d3d_com(&mut v, buffer, 15, &[1]), 0x80070057);
    assert_eq!(d3d_com(&mut v, buffer, 15, &[(-600i32) as u32]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 6, &[DATA + 0x930]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x930).unwrap() as i32, -600);
    assert_eq!(d3d_com(&mut v, buffer, 12, &[0, 0, 1]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 9, &[DATA + 0x930]), 0);
    assert_eq!(v.mem.read_u32(DATA + 0x930).unwrap(), 5);
    assert_eq!(d3d_com(&mut v, buffer, 18, &[]), 0);
    assert_eq!(d3d_com(&mut v, buffer, 2, &[]), 0);
    assert_eq!(
        v.k("HeapSize", &[v.state.process_heap, 0, second]),
        u32::MAX
    );
    v.mem.write_u32(DATA + 4, 16).unwrap();
    assert_eq!(
        d3d_com(&mut v, sound, 3, &[DATA, DATA + 0x910, 0]),
        0x80004001
    );
    assert_eq!(v.mem.read_u32(DATA + 0x910).unwrap(), 0);
    assert_eq!(d3d_com(&mut v, sound, 2, &[]), 0);
}
