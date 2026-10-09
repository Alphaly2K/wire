//! Moving the address space between cooperative host owners remains supported.
use wire::x64::{
    cpu::Cpu64,
    memory::{Memory, EXECUTE, PAGE, READ, WRITE},
};
#[test]
fn warmed_translations_and_checked_segments_move_between_owner_threads() {
    const PC: u64 = 0x140000000;
    const DATA: u64 = 0x20000;
    let mut mem = Memory::default();
    mem.map(PC, PAGE, READ | WRITE | EXECUTE).unwrap();
    mem.map(DATA, PAGE, READ | WRITE).unwrap();
    mem.write(
        PC,
        &[
            0x48, 0x8b, 0x07, 0x48, 0x83, 0xc0, 1, 0x48, 0x89, 0x47, 8, 0x48, 0x39, 0xc3,
        ],
    )
    .unwrap();
    let registers = [
        0x48, 0x01, 0xd8, 0x48, 0x31, 0xd1, 0x49, 0x89, 0xc2, 0x48, 0x39, 0xd8,
    ];
    mem.write(PC + 64, &registers).unwrap();
    mem.write_int(DATA, 8, 7).unwrap();
    let mut cpu = Cpu64::new(PC, 0);
    cpu.regs[7] = DATA;
    cpu.decode(&mem).unwrap();
    cpu.rip = PC + 64;
    cpu.decode(&mem).unwrap();
    assert_eq!(cpu.step_bounded(&mut mem, 4).unwrap(), 4);
    cpu.rip = PC;
    run_four(&mut cpu, &mut mem);
    let (mut cpu, mut mem) = std::thread::spawn(move || {
        cpu.rip = PC;
        run_four(&mut cpu, &mut mem);
        (cpu, mem)
    })
    .join()
    .unwrap();
    assert_eq!((cpu.retired, cpu.work), (12, 12));
    assert_eq!(mem.read_int(DATA + 8, 8).unwrap(), 8);
    cpu.rip = PC;
    run_four(&mut cpu, &mut mem);
    assert_eq!((cpu.retired, cpu.work), (16, 16));
}

fn run_four(cpu: &mut Cpu64, mem: &mut Memory) {
    let end = cpu.work + 4;
    while cpu.work < end {
        cpu.step_bounded(mem, end - cpu.work).unwrap();
    }
}
