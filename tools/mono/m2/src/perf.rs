//! Repeatable pure managed checksum workload for interpreter configuration
//! comparisons. This is not a real-game or native-vs-original Mono benchmark.
use super::*;
pub(super) fn run(mono: &Mono) {
    let method = mono.find("PureCompute", 1);
    let unbox = symbol!(
        mono.library,
        "mono_object_unbox",
        unsafe extern "C" fn(Object) -> Object
    );
    let mut count = 20_000_000i32;
    let mut args = [(&mut count as *mut i32).cast()];
    let mut expected = 7i32;
    for i in 0..count {
        expected = expected.wrapping_mul(33) ^ i;
        expected = expected.wrapping_add((i & 255) * 3 + 1);
    }
    let warm = mono.invoke(method, args.as_mut_ptr());
    assert_eq!(
        unsafe { unbox(warm).cast::<i32>().read_unaligned() },
        expected
    );
    for trial in 0..7 {
        let start = std::time::Instant::now();
        let boxed = mono.invoke(method, args.as_mut_ptr());
        let elapsed = start.elapsed().as_secs_f64();
        assert_eq!(
            unsafe { unbox(boxed).cast::<i32>().read_unaligned() },
            expected
        );
        println!("PURE_IL_TIMING trial={trial} iterations={count} checksum={expected} wall_s={elapsed:.9}");
    }
    println!("PASS pure IL checksum timing; native_game_backend_enabled=0");
}
